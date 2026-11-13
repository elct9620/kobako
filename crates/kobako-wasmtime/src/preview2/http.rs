//! What the Wasm Driver answers each `wasi:http` import a client makes. A
//! request the guest builds is handed to the Invocation's namespace, which
//! judges it on the guest thread and sends it through the outbound seam; until
//! a Grant gives the guest the network, `handle` answers `HTTP-request-denied`,
//! as a host that sends nothing does.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kobako_runtime::memcg::MemoryCharge;
use kobako_runtime::net::{self, Exchange, FieldError, RequestBody};

use super::bindings::wasi_http_types_0_2_12::{
    ErrorCode, Fields, FutureIncomingResponse, FutureTrailers, HeaderError, IncomingBody,
    IncomingResponse, Method, OutgoingBody, OutgoingRequest, RequestOptions, Scheme,
};
use super::bindings::wasi_io_poll_0_2_12::Pollable;
use super::bindings::wasi_io_streams_0_2_12::{InputStream, OutputStream};
use super::bindings::HttpHost;
use super::error;
use super::table::SharedFile;
use super::table::{self, Stream};
use crate::invocation::Invocation;

/// A request's body and whether the guest ended it, shared between the request
/// and the `outgoing-body` its `body` hands out.
pub(crate) struct BodyLink {
    body: Arc<RequestBody>,
    handed: AtomicBool,
    finished: AtomicBool,
    /// Whether the guest dropped the body without finishing it, which fails
    /// its request, as wasmtime's `abort` does.
    aborted: AtomicBool,
}

impl BodyLink {
    /// Whether the request may leave: its body was never asked for, or was
    /// ended.
    fn is_complete(&self) -> bool {
        !self.handed.load(Ordering::Acquire) || self.finished.load(Ordering::Acquire)
    }

    fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Acquire)
    }
}

/// A request the guest is building.
pub(crate) struct Request {
    method: Method,
    scheme: Option<Scheme>,
    authority: Option<String>,
    path_with_query: Option<String>,
    headers: Arc<net::Fields>,
    link: Arc<BodyLink>,
    /// What the method, authority, and path the guest set hold, reserved
    /// against the Invocation's memory, as its headers' room is.
    memory: MemoryCharge,
    /// The options `handle` was given.
    timeouts: net::Timeouts,
}

impl Request {
    /// The bytes the guest's own strings hold.
    fn size(&self) -> usize {
        let method = match &self.method {
            Method::Other(other) => other.len(),
            _ => 0,
        };
        method
            + self.authority.as_ref().map_or(0, String::len)
            + self.path_with_query.as_ref().map_or(0, String::len)
    }
}

/// Where a `future-incoming-response` stands.
pub(crate) enum FutureState {
    /// Handed over before its body ended, so it leaves once it does.
    Waiting(Request),
    /// Sent, with the file its response's arrival wakes, made as it leaves.
    Sent(Arc<Exchange>, SharedFile),
    Refused(ErrorCode),
    /// Its response was taken.
    Taken,
}

/// The host type each `wasi:http` resource the guest holds stands for, named
/// as its WIT resource is, apart from the bindings' handles of those names.
pub(crate) mod resource {
    use std::sync::Arc;

    use kobako_runtime::net::{self, Exchange};

    use super::{BodyLink, FutureState, Request};

    pub(crate) struct Fields {
        pub(crate) entries: Arc<net::Fields>,
        pub(crate) mutable: bool,
    }

    pub(crate) struct OutgoingRequest(pub(crate) Request);

    pub(crate) struct OutgoingBody {
        pub(crate) link: Arc<BodyLink>,
        /// Whether its stream was handed out.
        pub(crate) stream_handed: bool,
    }

    pub(crate) struct FutureIncomingResponse(pub(crate) FutureState);

    /// The timeouts a request is handled with.
    #[derive(Default)]
    pub(crate) struct RequestOptions(pub(crate) net::Timeouts);

    /// A response whose head its exchange holds once, against the
    /// Invocation's memory.
    pub(crate) struct IncomingResponse {
        pub(crate) status: u16,
        pub(crate) exchange: Arc<Exchange>,
        /// Whether its body was handed out.
        pub(crate) consumed: bool,
    }

    pub(crate) struct IncomingBody {
        pub(crate) exchange: Arc<Exchange>,
        /// Whether its stream was handed out.
        pub(crate) stream_handed: bool,
    }

    pub(crate) struct FutureTrailers;
}

/// The `header-error` a refused header answers.
fn to_header_error(err: FieldError) -> HeaderError {
    match err {
        FieldError::Forbidden => HeaderError::Forbidden,
        _ => HeaderError::InvalidSyntax,
    }
}

/// The trap a header list past its room ends the run with, since
/// `header-error` has no case for it.
fn to_field_trap(err: FieldError) -> wasmtime::Error {
    match err {
        FieldError::OutOfMemory => wasmtime::Error::msg("the guest's headers outgrow its memory"),
        _ => wasmtime::Error::msg("the guest's headers outgrow the most one list may hold"),
    }
}

/// `timeout` as the nanoseconds a WASI `duration` counts.
fn to_nanos(timeout: Option<Duration>) -> Option<u64> {
    timeout.map(|timeout| u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX))
}

/// The error code a request's failure answers, carrying no host text.
fn to_error_code(err: net::HttpError) -> ErrorCode {
    match err {
        net::HttpError::Denied => ErrorCode::HTTPRequestDenied,
        net::HttpError::DestinationNotFound => ErrorCode::DestinationNotFound,
        net::HttpError::ConnectionRefused => ErrorCode::ConnectionRefused,
        net::HttpError::ConnectionTimeout => ErrorCode::ConnectionTimeout,
        net::HttpError::ConnectionReadTimeout => ErrorCode::ConnectionReadTimeout,
        net::HttpError::ConnectionTerminated => ErrorCode::ConnectionTerminated,
        net::HttpError::ProtocolError => ErrorCode::HTTPProtocolError,
        net::HttpError::ConnectionLimitReached => ErrorCode::ConnectionLimitReached,
        net::HttpError::TlsCertificate => ErrorCode::TLSCertificateError,
        net::HttpError::TlsProtocol => ErrorCode::TLSProtocolError,
        _ => ErrorCode::InternalError(None),
    }
}

fn to_method_name(method: &Method) -> Option<String> {
    let name = match method {
        Method::Get => "GET",
        Method::Head => "HEAD",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
        Method::Connect => "CONNECT",
        Method::Options => "OPTIONS",
        Method::Trace => "TRACE",
        Method::Patch => "PATCH",
        Method::Other(other) => return net::is_token(other).then(|| other.clone()),
    };
    Some(name.to_string())
}

impl Invocation {
    fn request_mut(&mut self, request: OutgoingRequest) -> wasmtime::Result<&mut Request> {
        self.table_mut()
            .get_mut(request)
            .map(|request| &mut request.0)
    }

    /// Change `request` through `change`, then reserve what its strings now
    /// hold; past the Invocation's memory the call traps, as a header list's
    /// does.
    fn change_request(
        &mut self,
        request: OutgoingRequest,
        change: impl FnOnce(&mut Request),
    ) -> wasmtime::Result<()> {
        let request = self.request_mut(request)?;
        change(request);
        let size = request.size();
        request
            .memory
            .resize(size)
            .map_err(|_| wasmtime::Error::msg("the guest's request outgrows its memory"))
    }

    fn future_mut(&mut self, future: FutureIncomingResponse) -> wasmtime::Result<&mut FutureState> {
        self.table_mut().get_mut(future).map(|future| &mut future.0)
    }

    /// Hand `request` to the namespace, which judges it here on the guest
    /// thread before the reactor sends it.
    fn send(&mut self, request: Request) -> FutureState {
        let Some(method) = to_method_name(&request.method) else {
            return FutureState::Refused(ErrorCode::HTTPRequestMethodInvalid);
        };
        let scheme = match request.scheme.as_ref() {
            Some(Scheme::HTTP) => net::Scheme::Http,
            Some(Scheme::HTTPS) | None => net::Scheme::Https,
            Some(Scheme::Other(other)) => match other.as_str() {
                "http" => net::Scheme::Http,
                "https" => net::Scheme::Https,
                _ => return FutureState::Refused(ErrorCode::HTTPProtocolError),
            },
        };
        let Some(authority) = request.authority else {
            return FutureState::Refused(ErrorCode::HTTPRequestURIInvalid);
        };
        let mut outgoing = net::OutgoingRequest::new(method, scheme, authority);
        if let Some(path) = request.path_with_query {
            outgoing.path_with_query = path;
        }
        outgoing.headers = request.headers.entries();
        outgoing.body = request.link.body.take();
        outgoing.timeouts = request.timeouts;
        // The exchange reserves the request's parts itself, so the guest's
        // copies give theirs back before it does.
        drop((request.headers, request.memory));
        let Some(namespace) = self.wasi().network() else {
            return FutureState::Refused(ErrorCode::HTTPRequestDenied);
        };
        let cred = self.wasi().cred();
        match net::send(namespace, cred, outgoing) {
            Ok(exchange) => {
                let file = Arc::new(Mutex::new(exchange.alloc_response_file(cred)));
                FutureState::Sent(exchange, file)
            }
            Err(err) => FutureState::Refused(to_error_code(err)),
        }
    }

    /// The future's request leaves once its body ended, and fails once its body
    /// was dropped unfinished.
    fn advance(&mut self, future: FutureIncomingResponse) -> wasmtime::Result<()> {
        let state = self.future_mut(future)?;
        if matches!(state, FutureState::Waiting(request) if request.link.is_aborted()) {
            *state = FutureState::Refused(ErrorCode::HTTPProtocolError);
            return Ok(());
        }
        if !matches!(state, FutureState::Waiting(request) if request.link.is_complete()) {
            return Ok(());
        }
        let FutureState::Waiting(request) = std::mem::replace(state, FutureState::Taken) else {
            unreachable!("matched as waiting above");
        };
        let sent = self.send(request);
        *self.future_mut(future)? = sent;
        Ok(())
    }
}

impl HttpHost for Invocation {
    fn types_convert_error_code(&mut self, err: error::HttpError) -> wasmtime::Result<ErrorCode> {
        err.downcast()
    }

    fn types_convert_header_error(
        &mut self,
        err: error::HeaderError,
    ) -> wasmtime::Result<HeaderError> {
        err.downcast()
    }

    fn types_fields_new(&mut self) -> wasmtime::Result<Fields> {
        let entries = net::Fields::new(self.wasi().cred().clone());
        self.table_mut().push(resource::Fields {
            entries,
            mutable: true,
        })
    }

    /// A header appended, or the `header-error` refusing it; a list past
    /// `MAX_FIELD_SIZE` or the Invocation's memory traps, as wasmtime's
    /// field limit does.
    fn types_fields_append(
        &mut self,
        this: Fields,
        name: String,
        value: Vec<u8>,
    ) -> Result<(), error::HeaderError> {
        let fields = self.table_mut().get(this)?;
        if !fields.mutable {
            // The header is read before the list is found immutable, as
            // wasmtime parses one before it asks whether it may change.
            let read = net::to_field_name(&name).and_then(|_| net::to_field_value(value));
            return Err(read
                .map_or_else(to_header_error, |_| HeaderError::Immutable)
                .into());
        }
        match fields.entries.append(&name, value) {
            Ok(()) => Ok(()),
            Err(err @ (FieldError::TooLarge | FieldError::OutOfMemory)) => {
                Err(to_field_trap(err).into())
            }
            Err(err) => Err(to_header_error(err).into()),
        }
    }

    fn types_fields_entries(&mut self, this: Fields) -> wasmtime::Result<Vec<(String, Vec<u8>)>> {
        Ok(self.table_mut().get(this)?.entries.entries())
    }

    fn types_fields_drop(&mut self, handle: Fields) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// A GET with `headers`, which it takes, and a body the guest may ask for.
    fn types_outgoing_request_new(
        &mut self,
        headers: resource::Fields,
    ) -> wasmtime::Result<OutgoingRequest> {
        let entries = headers.entries;
        let cred = self.wasi().cred().clone();
        let memory = MemoryCharge::empty(cred.memory_budget());
        let link = Arc::new(BodyLink {
            body: RequestBody::new(cred),
            handed: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            aborted: AtomicBool::new(false),
        });
        self.table_mut().push(resource::OutgoingRequest(Request {
            method: Method::Get,
            scheme: None,
            authority: None,
            path_with_query: None,
            headers: entries,
            link,
            memory,
            timeouts: net::Timeouts::default(),
        }))
    }

    /// The body, handed out once.
    fn types_outgoing_request_body(
        &mut self,
        this: OutgoingRequest,
    ) -> wasmtime::Result<Result<OutgoingBody, ()>> {
        let link = self.request_mut(this)?.link.clone();
        if link.handed.swap(true, Ordering::AcqRel) {
            return Ok(Err(()));
        }
        self.table_mut()
            .push(resource::OutgoingBody {
                link,
                stream_handed: false,
            })
            .map(Ok)
    }

    fn types_outgoing_request_set_method(
        &mut self,
        this: OutgoingRequest,
        method: Method,
    ) -> wasmtime::Result<Result<(), ()>> {
        if to_method_name(&method).is_none() {
            return Ok(Err(()));
        }
        self.change_request(this, |request| request.method = method)?;
        Ok(Ok(()))
    }

    fn types_outgoing_request_set_path_with_query(
        &mut self,
        this: OutgoingRequest,
        path_with_query: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let target = match path_with_query.as_deref().map(net::to_origin_form) {
            Some(None) => return Ok(Err(())),
            target => target.flatten(),
        };
        self.change_request(this, |request| request.path_with_query = target)?;
        Ok(Ok(()))
    }

    /// Any scheme hyper parses, as wasmtime's; one that is neither HTTP nor
    /// HTTPS answers `HTTP-protocol-error` once the request is sent.
    fn types_outgoing_request_set_scheme(
        &mut self,
        this: OutgoingRequest,
        scheme: Option<Scheme>,
    ) -> wasmtime::Result<Result<(), ()>> {
        if let Some(Scheme::Other(other)) = &scheme {
            if !net::is_scheme(other) {
                return Ok(Err(()));
            }
        }
        self.request_mut(this)?.scheme = scheme;
        Ok(Ok(()))
    }

    fn types_outgoing_request_set_authority(
        &mut self,
        this: OutgoingRequest,
        authority: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        if authority
            .as_deref()
            .is_some_and(|authority| !net::is_authority(authority))
        {
            return Ok(Err(()));
        }
        self.change_request(this, |request| request.authority = authority)?;
        Ok(Ok(()))
    }

    fn types_outgoing_request_drop(&mut self, handle: OutgoingRequest) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// The stream writing the body, handed out once.
    fn types_outgoing_body_write(
        &mut self,
        this: OutgoingBody,
    ) -> wasmtime::Result<Result<OutputStream, ()>> {
        let body = self.table_mut().get_mut(this)?;
        if std::mem::replace(&mut body.stream_handed, true) {
            return Ok(Err(()));
        }
        let stream = Stream::File(Arc::new(Mutex::new(body.link.body.alloc_file())));
        self.table_mut()
            .push_child(table::OutputStream::new(stream), this)
            .map(Ok)
    }

    /// End the body, so its request may leave. Trailers holding a field
    /// answer `HTTP-request-denied` and fail the request: an egress proxy
    /// sends none by default, as Envoy drops HTTP/1 trailers, and the request
    /// policies never see them, so dropping them would succeed differently.
    fn types_outgoing_body_finish(
        &mut self,
        this: resource::OutgoingBody,
        trailers: Option<resource::Fields>,
    ) -> Result<(), error::HttpError> {
        if trailers.is_some_and(|trailers| !trailers.entries.entries().is_empty()) {
            this.link.aborted.store(true, Ordering::Release);
            return Err(ErrorCode::HTTPRequestDenied.into());
        }
        this.link.finished.store(true, Ordering::Release);
        Ok(())
    }

    /// A body dropped unfinished fails its request, as wasmtime-wasi-http aborts
    /// it with an HTTP protocol error.
    fn types_outgoing_body_drop(&mut self, handle: OutgoingBody) -> wasmtime::Result<()> {
        let link = self.table_mut().delete(handle)?.link;
        link.aborted.store(true, Ordering::Release);
        Ok(())
    }

    /// Ready once the response's head arrives; one whose request has not left
    /// yet, or that already answered, is ready at once.
    fn types_future_incoming_response_subscribe(
        &mut self,
        this: FutureIncomingResponse,
    ) -> wasmtime::Result<Pollable> {
        self.advance(this)?;
        let pollable = match self.future_mut(this)? {
            FutureState::Sent(exchange, file) => {
                table::Pollable::Exchange(file.clone(), exchange.clone())
            }
            _ => table::Pollable::Ready,
        };
        self.table_mut().push_child(pollable, this)
    }

    /// The response once its head arrives, handed out once.
    #[allow(clippy::type_complexity)]
    fn types_future_incoming_response_get(
        &mut self,
        this: FutureIncomingResponse,
    ) -> wasmtime::Result<Option<Result<Result<IncomingResponse, ErrorCode>, ()>>> {
        self.advance(this)?;
        let state = self.future_mut(this)?;
        let answer = match state {
            FutureState::Waiting(_) => return Ok(None),
            FutureState::Taken => return Ok(Some(Err(()))),
            FutureState::Refused(code) => Err(code.clone()),
            FutureState::Sent(exchange, _) => match exchange.response() {
                None => return Ok(None),
                Some(Ok(head)) => Ok((head.status, exchange.clone())),
                Some(Err(err)) => Err(to_error_code(err)),
            },
        };
        *state = FutureState::Taken;
        match answer {
            Ok((status, exchange)) => self
                .table_mut()
                .push(resource::IncomingResponse {
                    status,
                    exchange,
                    consumed: false,
                })
                .map(|response| Some(Ok(Ok(response)))),
            Err(code) => Ok(Some(Ok(Err(code)))),
        }
    }

    fn types_future_incoming_response_drop(
        &mut self,
        handle: FutureIncomingResponse,
    ) -> wasmtime::Result<()> {
        // A sent future holds its exchange's last reference beside its own
        // child pollable, so dropping it lets the exchange go.
        self.table_mut().delete(handle).map(drop)
    }

    fn types_incoming_response_status(&mut self, this: IncomingResponse) -> wasmtime::Result<u16> {
        Ok(self.table_mut().get(this)?.status)
    }

    /// The response's headers, which the guest cannot change; a copy that
    /// outlives the response, as wasmtime's.
    fn types_incoming_response_headers(
        &mut self,
        this: IncomingResponse,
    ) -> wasmtime::Result<Fields> {
        let headers = match self.table_mut().get(this)?.exchange.response() {
            Some(Ok(head)) => head.headers,
            _ => unreachable!("a response is handed out only once its head arrived"),
        };
        let cred = self.wasi().cred().clone();
        let entries = net::Fields::with_entries(cred, headers).map_err(to_field_trap)?;
        self.table_mut().push(resource::Fields {
            entries,
            mutable: false,
        })
    }

    /// The body, handed out once.
    fn types_incoming_response_consume(
        &mut self,
        this: IncomingResponse,
    ) -> wasmtime::Result<Result<IncomingBody, ()>> {
        let response = self.table_mut().get_mut(this)?;
        if std::mem::replace(&mut response.consumed, true) {
            return Ok(Err(()));
        }
        let exchange = response.exchange.clone();
        self.table_mut()
            .push(resource::IncomingBody {
                exchange,
                stream_handed: false,
            })
            .map(Ok)
    }

    /// A response dropped with its body never asked for lets the exchange go.
    fn types_incoming_response_drop(&mut self, handle: IncomingResponse) -> wasmtime::Result<()> {
        let response = self.table_mut().delete(handle)?;
        if !response.consumed {
            response.exchange.release();
        }
        Ok(())
    }

    /// The stream reading the body, handed out once.
    fn types_incoming_body_stream(
        &mut self,
        this: IncomingBody,
    ) -> wasmtime::Result<Result<InputStream, ()>> {
        let body = self.table_mut().get_mut(this)?;
        if std::mem::replace(&mut body.stream_handed, true) {
            return Ok(Err(()));
        }
        let exchange = body.exchange.clone();
        let file = Arc::new(Mutex::new(exchange.alloc_body_file(self.wasi().cred())));
        self.table_mut()
            .push_child(
                table::InputStream::of_body(Stream::File(file), exchange),
                this,
            )
            .map(Ok)
    }

    /// The trailers, which the outbound seam never reads, so none arrive.
    fn types_incoming_body_finish(
        &mut self,
        this: resource::IncomingBody,
    ) -> wasmtime::Result<FutureTrailers> {
        this.exchange.release();
        self.table_mut().push(resource::FutureTrailers)
    }

    fn types_incoming_body_drop(&mut self, handle: IncomingBody) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    fn types_future_trailers_drop(&mut self, handle: FutureTrailers) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// Options waiting on at every step until one is set.
    fn types_request_options_new(&mut self) -> wasmtime::Result<RequestOptions> {
        self.table_mut().push(resource::RequestOptions::default())
    }

    fn types_request_options_connect_timeout(
        &mut self,
        this: RequestOptions,
    ) -> wasmtime::Result<Option<u64>> {
        Ok(to_nanos(self.table_mut().get(this)?.0.connect))
    }

    /// Bound the connect, as wasmtime-wasi-http bounds it, so a request
    /// whose connect outlasts it answers `connection-timeout`.
    fn types_request_options_set_connect_timeout(
        &mut self,
        this: RequestOptions,
        duration: Option<u64>,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.table_mut().get_mut(this)?.0.connect = duration.map(Duration::from_nanos);
        Ok(Ok(()))
    }

    fn types_request_options_first_byte_timeout(
        &mut self,
        this: RequestOptions,
    ) -> wasmtime::Result<Option<u64>> {
        Ok(to_nanos(self.table_mut().get(this)?.0.first_byte))
    }

    /// Bound the wait for the response's head once connected, answered as
    /// `connection-read-timeout`.
    fn types_request_options_set_first_byte_timeout(
        &mut self,
        this: RequestOptions,
        duration: Option<u64>,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.table_mut().get_mut(this)?.0.first_byte = duration.map(Duration::from_nanos);
        Ok(Ok(()))
    }

    fn types_request_options_between_bytes_timeout(
        &mut self,
        this: RequestOptions,
    ) -> wasmtime::Result<Option<u64>> {
        Ok(to_nanos(self.table_mut().get(this)?.0.between_bytes))
    }

    /// Bound each wait for the body's next part, failing the read that
    /// outlasts it.
    fn types_request_options_set_between_bytes_timeout(
        &mut self,
        this: RequestOptions,
        duration: Option<u64>,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.table_mut().get_mut(this)?.0.between_bytes = duration.map(Duration::from_nanos);
        Ok(Ok(()))
    }

    fn types_request_options_drop(&mut self, handle: RequestOptions) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// Send `request`, which it takes. Without a network Grant it answers
    /// `HTTP-request-denied` at once, doing nothing first; a request whose body
    /// has not ended leaves once it does, bound by the timeouts `options`
    /// sets.
    fn outgoing_handler_handle(
        &mut self,
        request: resource::OutgoingRequest,
        options: Option<resource::RequestOptions>,
    ) -> Result<FutureIncomingResponse, error::HttpError> {
        let mut request = request.0;
        request.timeouts = options.unwrap_or_default().0;
        if self.wasi().network().is_none() {
            return Err(ErrorCode::HTTPRequestDenied.into());
        }
        if self.wasi().table().is_full() {
            return Err(ErrorCode::ConnectionLimitReached.into());
        }
        let future = match request.link.is_complete() {
            true => self.send(request),
            false => FutureState::Waiting(request),
        };
        if let FutureState::Refused(code) = future {
            return Err(code.into());
        }
        Ok(self
            .table_mut()
            .push_file(resource::FutureIncomingResponse(future))?)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::time::{Duration as Wait, Instant};

    use kobako_runtime::network;
    use kobako_runtime::sched::run_task;

    use super::super::IntoAnswer;
    use super::*;
    use crate::preview2::bindings::{Host as _, IoHost};

    /// A hermetic invocation whose network holds `endpoints`, or none at all.
    fn guest(endpoints: Option<Vec<network::Endpoint>>) -> Invocation {
        limited_guest(endpoints, None)
    }

    /// A guest as `guest` makes one, allotted `memory_limit` bytes.
    fn limited_guest(
        endpoints: Option<Vec<network::Endpoint>>,
        memory_limit: Option<usize>,
    ) -> Invocation {
        Invocation::hermetic(endpoints, |config| config.memory_limit = memory_limit)
    }

    /// A GET for `/greeting` at `authority` over HTTP.
    fn greeting_request(
        guest: &mut Invocation,
        authority: &str,
    ) -> wasmtime::Result<OutgoingRequest> {
        let headers = guest.types_fields_new()?;
        let headers = guest.take_handle(headers)?;
        let request = guest.types_outgoing_request_new(headers)?;
        guest
            .types_outgoing_request_set_scheme(request, Some(Scheme::HTTP))?
            .expect("http is a scheme");
        guest
            .types_outgoing_request_set_authority(request, Some(authority.to_string()))?
            .expect("an authority");
        guest
            .types_outgoing_request_set_path_with_query(request, Some("/greeting".to_string()))?
            .expect("a path");
        Ok(request)
    }

    // @behavior NW-039
    #[test]
    fn without_the_network_a_request_answers_http_request_denied() {
        let mut guest = guest(None);
        let request = greeting_request(&mut guest, "example.com").unwrap();
        let request = guest.take_handle(request).unwrap();
        assert!(
            matches!(
                guest.outgoing_handler_handle(request, None).into_answer(),
                Ok(Err(ErrorCode::HTTPRequestDenied))
            ),
            "outgoing-handler.handle with no network entry must answer HTTP-request-denied"
        );
    }

    // @behavior NW-206
    #[test]
    fn request_options_read_back_their_timeouts_and_bound_the_wait_for_the_head() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            // Read the request, then hold the connection with no answer
            // until the client lets it go.
            let mut reader = BufReader::new(stream);
            while reader.read_line(&mut line).unwrap_or(0) > 0 {}
        });
        let endpoint = network::Endpoint::new("127.0.0.1", port).unwrap();
        let mut guest = guest(Some(vec![endpoint]));
        let started = Instant::now();
        let (read_back, answer) = run_task(Some(started + Wait::from_secs(10)), || {
            let options = guest.types_request_options_new()?;
            let unset = guest.types_request_options_first_byte_timeout(options)?;
            let set = [
                guest.types_request_options_set_connect_timeout(options, Some(7))?,
                guest.types_request_options_set_first_byte_timeout(options, Some(0))?,
                guest.types_request_options_set_between_bytes_timeout(options, Some(9))?,
            ];
            let read_back = (
                unset,
                set,
                guest.types_request_options_connect_timeout(options)?,
                guest.types_request_options_between_bytes_timeout(options)?,
            );
            guest
                .types_request_options_set_connect_timeout(options, None)?
                .unwrap();
            let options = guest.take_handle(options)?;
            let request = greeting_request(&mut guest, &format!("127.0.0.1:{port}"))?;
            let request = guest.take_handle(request)?;
            let future = guest
                .outgoing_handler_handle(request, Some(options))
                .into_answer()?
                .expect("an endpoint names the authority");
            let ready = guest.types_future_incoming_response_subscribe(future)?;
            guest.poll_pollable_block(ready)?;
            let answer = guest
                .types_future_incoming_response_get(future)?
                .expect("the wait ended with an answer")
                .expect("the answer is handed out once")
                .err();
            guest.poll_pollable_drop(ready)?;
            guest.types_future_incoming_response_drop(future)?;
            Ok::<_, wasmtime::Error>((read_back, answer))
        })
        .expect("the calls answer");
        let woke_by_itself = started.elapsed() < Wait::from_secs(5);
        server.join().unwrap();
        assert_eq!(
            (read_back, answer, woke_by_itself),
            (
                (None, [Ok(()), Ok(()), Ok(())], Some(7), Some(9)),
                Some(ErrorCode::ConnectionReadTimeout),
                true
            ),
            "request-options must read back each timeout set, none until set, and a request \
             handled with a first-byte timeout must wake its future's pollable once the timeout \
             passes, not at the run's deadline, and answer connection-read-timeout"
        );
    }

    // @behavior NW-040
    // @behavior NW-165
    #[test]
    fn a_request_through_wasi_http_reads_back_the_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            (&stream)
                .write_all(
                    b"HTTP/1.1 201 Created\r\ncontent-length: 5\r\nx-kind: test\r\n\r\nhello",
                )
                .unwrap();
            line
        });
        let endpoint = network::Endpoint::new("127.0.0.1", port).unwrap();
        let mut guest = guest(Some(vec![endpoint]));
        let started = Instant::now();
        let (status, kind, body, nothing) = run_task(Some(started + Wait::from_secs(10)), || {
            let request = greeting_request(&mut guest, &format!("127.0.0.1:{port}"))?;
            let request = guest.take_handle(request)?;
            let future = guest
                .outgoing_handler_handle(request, None)
                .into_answer()?
                .expect("an endpoint names the authority");
            let ready = guest.types_future_incoming_response_subscribe(future)?;
            guest.poll_pollable_block(ready)?;
            let response = guest
                .types_future_incoming_response_get(future)?
                .expect("the head arrived")
                .expect("handed out once")
                .expect("the server answered");
            let status = guest.types_incoming_response_status(response)?;
            let headers = guest.types_incoming_response_headers(response)?;
            let kind = guest
                .types_fields_entries(headers)?
                .into_iter()
                .filter(|(name, _)| name == "x-kind")
                .map(|(_, value)| value)
                .collect::<Vec<_>>();
            let body = guest
                .types_incoming_response_consume(response)?
                .expect("consumed once");
            let stream = guest.types_incoming_body_stream(body)?.expect("one stream");
            let mut read = Vec::new();
            while let Ok(bytes) = guest.streams_input_stream_read(stream, 64).into_answer()? {
                if bytes.is_empty() {
                    let readable = guest.streams_input_stream_subscribe(stream)?;
                    guest.poll_pollable_block(readable)?;
                    guest.poll_pollable_drop(readable)?;
                }
                read.extend(bytes);
            }
            let nothing = matches!(
                guest.streams_input_stream_read(stream, 0).into_answer()?,
                Err(crate::preview2::error::StreamError::Closed)
            );
            Ok::<_, wasmtime::Error>((status, kind, read, nothing))
        })
        .expect("the request answers");
        assert_eq!(
            (status, kind, body, server.join().unwrap(), nothing),
            (
                201,
                vec![b"test".to_vec()],
                b"hello".to_vec(),
                "GET /greeting HTTP/1.1\r\n".to_string(),
                true
            ),
            "a GET built through wasi:http to an http endpoint must reach the server and read \
             back the status, a header under its lowercase name, and the body to its end; a \
             read of nothing past the end must answer closed, as io.wit has it"
        );
    }

    // @behavior NW-082
    #[test]
    fn an_exchange_the_guest_no_longer_holds_closes_its_connection() {
        // Whether the server reads the connection closed within a few
        // seconds once `leave` lets go of a response whose body stays open.
        let closes = |leave: fn(&mut Option<Invocation>, IncomingBody)| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 2 {
                    line.clear();
                }
                (&stream)
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\npart")
                    .unwrap();
                stream.set_read_timeout(Some(Wait::from_secs(5))).unwrap();
                let mut rest = [0; 1];
                matches!(std::io::Read::read(&mut reader, &mut rest), Ok(0))
            });
            let endpoint = network::Endpoint::new("127.0.0.1", port).unwrap();
            let mut guest = Some(guest(Some(vec![endpoint])));
            let started = Instant::now();
            let body = run_task(Some(started + Wait::from_secs(10)), || {
                let guest = guest.as_mut().unwrap();
                let request = greeting_request(guest, &format!("127.0.0.1:{port}"))?;
                let request = guest.take_handle(request)?;
                let future = guest
                    .outgoing_handler_handle(request, None)
                    .into_answer()?
                    .expect("an endpoint names the authority");
                let ready = guest.types_future_incoming_response_subscribe(future)?;
                guest.poll_pollable_block(ready)?;
                guest.poll_pollable_drop(ready)?;
                let response = guest
                    .types_future_incoming_response_get(future)?
                    .expect("the head arrived")
                    .expect("handed out once")
                    .expect("the server answered");
                guest.types_future_incoming_response_drop(future)?;
                let body = guest
                    .types_incoming_response_consume(response)?
                    .expect("consumed once");
                guest.types_incoming_response_drop(response)?;
                Ok::<_, wasmtime::Error>(body)
            })
            .expect("the head answers");
            leave(&mut guest, body);
            server.join().unwrap()
        };
        let dropped = closes(|guest, body| {
            guest
                .as_mut()
                .unwrap()
                .types_incoming_body_drop(body)
                .unwrap();
        });
        let ended = closes(|guest, _| {
            guest.take();
        });
        assert_eq!(
            (dropped, ended),
            (true, true),
            "a server holding a response body open must read the connection closed once the \
             guest drops the body unfinished, and once the Invocation ends holding it"
        );
    }

    // @behavior NW-041
    #[test]
    fn a_forbidden_header_or_a_response_header_cannot_be_set() {
        let mut guest = guest(None);
        let fields = guest.types_fields_new().unwrap();
        let set = |guest: &mut Invocation, fields, name: &str| {
            guest
                .types_fields_append(fields, name.to_string(), b"x".to_vec())
                .into_answer()
                .unwrap()
        };
        let answers = [
            set(&mut guest, fields, "Host"),
            set(&mut guest, fields, "content-length"),
            set(&mut guest, fields, "bad name"),
            set(&mut guest, fields, "x-ok"),
        ];
        let entries = net::Fields::new(guest.wasi().cred().clone());
        let frozen = guest.table_mut().push(resource::Fields {
            entries,
            mutable: false,
        });
        let immutable = set(&mut guest, frozen.unwrap(), "x-ok");
        assert_eq!(
            (answers, immutable),
            (
                [
                    Err(HeaderError::Forbidden),
                    Err(HeaderError::Forbidden),
                    Err(HeaderError::InvalidSyntax),
                    Ok(())
                ],
                Err(HeaderError::Immutable)
            ),
            "fields.append must refuse a forbidden name, content-length, a name that is no \
             token, and any change to an immutable list"
        );
    }

    // @behavior NW-081
    #[test]
    fn a_header_list_past_its_room_or_the_guests_memory_traps() {
        // How many 1 KiB headers a new list takes before its trap, and the trap.
        let fill = |guest: &mut Invocation| {
            let fields = guest.types_fields_new().unwrap();
            let mut taken = 0;
            for i in 0..200 {
                if let Err(trap) = guest
                    .types_fields_append(fields, format!("x-{i}"), vec![b'v'; 1024])
                    .into_answer()
                {
                    guest.types_fields_drop(fields).unwrap();
                    return (taken, trap.to_string());
                }
                taken += 1;
            }
            (taken, "no trap".to_string())
        };
        let (_, roomy) = fill(&mut guest(None));
        let mut tight = limited_guest(None, Some(16 * 1024));
        let (first, short) = fill(&mut tight);
        let (again, _) = fill(&mut tight);
        assert_eq!(
            (roomy.as_str(), short.as_str(), first > 0 && again == first),
            (
                "the guest's headers outgrow the most one list may hold",
                "the guest's headers outgrow its memory",
                true
            ),
            "fields.append of 1 KiB values must trap once the list passes 128 KiB, and sooner \
             once it passes a 16 KiB memory_limit; a dropped list must give its room back, so \
             a new one takes as many"
        );
    }

    // @behavior NW-156
    #[test]
    fn the_paths_requests_hold_count_against_the_guests_memory() {
        // How many requests holding a 1 KiB path a guest builds before the
        // trap, and the trap; every request it built is dropped after.
        let fill = |guest: &mut Invocation| {
            let path = format!("/{}", "p".repeat(1023));
            let mut held = Vec::new();
            let trap = loop {
                if held.len() > 1000 {
                    break "no trap".to_string();
                }
                let headers = guest.types_fields_new().unwrap();
                let headers = guest.take_handle(headers).unwrap();
                let request = guest.types_outgoing_request_new(headers).unwrap();
                held.push(request);
                let set =
                    guest.types_outgoing_request_set_path_with_query(request, Some(path.clone()));
                if let Err(trap) = set {
                    break trap.to_string();
                }
            };
            let taken = held.len() - 1;
            for request in held {
                guest.types_outgoing_request_drop(request).unwrap();
            }
            (taken, trap)
        };
        let mut tight = limited_guest(None, Some(16 * 1024));
        let (first, trap) = fill(&mut tight);
        let (again, _) = fill(&mut tight);
        assert_eq!(
            (trap.as_str(), first > 0 && again == first),
            ("the guest's request outgrows its memory", true),
            "outgoing-request.set-path-with-query of 1 KiB paths must trap once the requests \
             holding them pass a 16 KiB memory_limit, and dropped requests must give their room \
             back, so as many are taken again; took {first} then {again}"
        );
    }

    // @behavior NW-093
    #[test]
    fn a_requests_parts_are_judged_as_they_are_set() {
        let mut guest = guest(None);
        let fields = guest.types_fields_new().unwrap();
        let control = guest
            .types_fields_append(fields, "x-ok".to_string(), b"a\x01b".to_vec())
            .into_answer()
            .unwrap();
        let fields = guest.take_handle(fields).unwrap();
        let request = guest.types_outgoing_request_new(fields).unwrap();
        let mut authority = |value: &str| {
            guest
                .types_outgoing_request_set_authority(request, Some(value.to_string()))
                .unwrap()
                .is_ok()
        };
        let authorities = [
            authority("example.com:"),
            authority("example.com:abc"),
            authority("example.com:80"),
        ];
        let mut path = |value: &str| {
            guest
                .types_outgoing_request_set_path_with_query(request, Some(value.to_string()))
                .unwrap()
                .ok()
                .map(|()| guest.request_mut(request).unwrap().path_with_query.clone())
        };
        let paths = [path(""), path("?q"), path("relative")];
        assert_eq!(
            (control, authorities, paths),
            (
                Err(HeaderError::InvalidSyntax),
                [false, false, true],
                [
                    Some(Some("/".to_string())),
                    Some(Some("/?q".to_string())),
                    None
                ]
            ),
            "fields.append of a value holding a control byte must answer invalid-syntax, \
             set-authority must refuse an empty or non-numeric port, and set-path-with-query \
             must take \"\" as / and \"?q\" as /?q, refusing a relative path"
        );
    }

    // @behavior NW-145
    #[test]
    fn a_rule_judges_the_target_sent_with_no_fragment() {
        let endpoint = network::Endpoint::new("example.com", 80)
            .unwrap()
            .with_rules(vec![network::HttpRule::new(
                network::Action::Deny,
                None,
                "/admin",
            )]);
        let mut guest = guest(Some(vec![endpoint]));
        let request = greeting_request(&mut guest, "example.com").unwrap();
        guest
            .types_outgoing_request_set_path_with_query(request, Some("/admin#x".to_string()))
            .unwrap()
            .expect("a path with a fragment");
        let target = guest.request_mut(request).unwrap().path_with_query.clone();
        let request = guest.take_handle(request).unwrap();
        let answer = guest.outgoing_handler_handle(request, None).into_answer();
        assert!(
            target.as_deref() == Some("/admin")
                && matches!(answer, Ok(Err(ErrorCode::HTTPRequestDenied))),
            "set-path-with-query must take /admin#x as /admin, the target a request sends, \
             and a rule denying /admin must refuse it; read back {target:?}"
        );
    }

    // @behavior NW-094
    #[test]
    fn an_immutable_list_reads_a_header_before_refusing_the_change() {
        let mut guest = guest(None);
        let entries = net::Fields::new(guest.wasi().cred().clone());
        let frozen = guest
            .table_mut()
            .push(resource::Fields {
                entries,
                mutable: false,
            })
            .unwrap();
        assert_eq!(
            guest
                .types_fields_append(frozen, "bad name".to_string(), b"x".to_vec())
                .into_answer()
                .unwrap(),
            Err(HeaderError::InvalidSyntax),
            "fields.append of a name that is no token to an immutable list must answer \
             invalid-syntax, as wasmtime reads the header before it finds the list immutable"
        );
    }

    // @behavior NW-064
    #[test]
    fn a_body_dropped_or_finished_while_its_stream_lives_traps() {
        let mut guest = guest(None);
        let request = greeting_request(&mut guest, "example.com").unwrap();
        let body = guest
            .types_outgoing_request_body(request)
            .unwrap()
            .expect("the body is handed out once");
        let stream = guest
            .types_outgoing_body_write(body)
            .unwrap()
            .expect("the stream is handed out once");
        let dropped = guest.types_outgoing_body_drop(body).is_err();
        // `finish` takes the body as its glue lifts it, so the lift is where
        // a living child traps.
        let finished = guest.take_handle(body).is_err();
        guest.streams_output_stream_drop(stream).unwrap();
        let taken = guest.take_handle(body).unwrap();
        let after = guest
            .types_outgoing_body_finish(taken, None)
            .into_answer()
            .unwrap();
        assert_eq!(
            (dropped, finished, after),
            (true, true, Ok(())),
            "outgoing-body.drop and finish must trap while the body's output-stream lives, \
             and finish once the stream is dropped"
        );
    }

    // @behavior NW-161
    #[test]
    fn a_responses_headers_outlive_the_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            (&stream)
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nx-kind: test\r\n\r\n")
                .unwrap();
        });
        let endpoint = network::Endpoint::new("127.0.0.1", port).unwrap();
        let mut guest = guest(Some(vec![endpoint]));
        let started = Instant::now();
        let (dropped, kind) = run_task(Some(started + Wait::from_secs(10)), || {
            let request = greeting_request(&mut guest, &format!("127.0.0.1:{port}"))?;
            let request = guest.take_handle(request)?;
            let future = guest
                .outgoing_handler_handle(request, None)
                .into_answer()?
                .expect("an endpoint names the authority");
            let ready = guest.types_future_incoming_response_subscribe(future)?;
            guest.poll_pollable_block(ready)?;
            guest.poll_pollable_drop(ready)?;
            let response = guest
                .types_future_incoming_response_get(future)?
                .expect("the head arrived")
                .expect("handed out once")
                .expect("the server answered");
            let headers = guest.types_incoming_response_headers(response)?;
            let dropped = guest.types_incoming_response_drop(response).is_ok();
            let kind = guest
                .types_fields_entries(headers)?
                .into_iter()
                .filter(|(name, _)| name == "x-kind")
                .map(|(_, value)| value)
                .collect::<Vec<_>>();
            Ok::<_, wasmtime::Error>((dropped, kind))
        })
        .expect("the request answers");
        server.join().unwrap();
        assert_eq!(
            (dropped, kind),
            (true, vec![b"test".to_vec()]),
            "incoming-response.drop while its headers live must not trap, and the headers must \
             still read back, as wasmtime hands out a copy"
        );
    }

    /// What sending a GET for `/` to `unnamed.example` answers, with `scheme`
    /// set; `None` when setting the scheme fails.
    fn send_unnamed(
        guest: &mut Invocation,
        scheme: Scheme,
    ) -> wasmtime::Result<Option<Result<(), ErrorCode>>> {
        let headers = guest.types_fields_new()?;
        let headers = guest.take_handle(headers)?;
        let request = guest.types_outgoing_request_new(headers)?;
        if guest
            .types_outgoing_request_set_scheme(request, Some(scheme))?
            .is_err()
        {
            return Ok(None);
        }
        guest
            .types_outgoing_request_set_authority(request, Some("unnamed.example".to_string()))?
            .expect("an authority");
        let request = guest.take_handle(request)?;
        let sent = guest.outgoing_handler_handle(request, None).into_answer()?;
        Ok(Some(sent.map(drop)))
    }

    // @behavior NW-162
    #[test]
    fn a_requests_scheme_is_judged_where_wasmtime_judges_it() {
        let mut guest = guest(Some(vec![
            network::Endpoint::new("example.com", 80).unwrap()
        ]));
        let other = |name: &str| Scheme::Other(name.to_string());
        let answers = [
            send_unnamed(&mut guest, other("not a scheme")),
            send_unnamed(&mut guest, other("ftp")),
            send_unnamed(&mut guest, other("http")),
        ]
        .map(|answer| answer.expect("the guest's calls must not trap"));
        assert_eq!(
            answers,
            [
                None,
                Some(Err(ErrorCode::HTTPProtocolError)),
                Some(Err(ErrorCode::HTTPRequestDenied))
            ],
            "outgoing-request.set-scheme must refuse only a scheme that does not parse; handle \
             must answer HTTP-protocol-error for ftp and judge http as HTTP, as wasmtime does"
        );
    }

    // @behavior NW-193
    #[test]
    fn a_body_finished_with_trailers_is_refused_and_fails_its_request() {
        let mut guest = guest(Some(vec![
            network::Endpoint::new("example.com", 80).unwrap()
        ]));
        // Whether `finish` with `trailers` refuses, and what the request's
        // future answers then.
        let mut finished_with = |trailer: Option<(&str, &[u8])>| {
            let request = greeting_request(&mut guest, "example.com").unwrap();
            let body = guest
                .types_outgoing_request_body(request)
                .unwrap()
                .expect("the body is handed out once");
            let request = guest.take_handle(request).unwrap();
            let future = guest
                .outgoing_handler_handle(request, None)
                .into_answer()
                .unwrap()
                .expect("the request waits on its body");
            let trailers = guest.types_fields_new().unwrap();
            if let Some((name, value)) = trailer {
                guest
                    .types_fields_append(trailers, name.to_string(), value.to_vec())
                    .into_answer()
                    .unwrap()
                    .expect("a trailer appends");
            }
            let trailers = guest.take_handle(trailers).unwrap();
            let body = guest.take_handle(body).unwrap();
            let finished = guest
                .types_outgoing_body_finish(body, Some(trailers))
                .into_answer()
                .unwrap();
            let answer = guest
                .types_future_incoming_response_get(future)
                .unwrap()
                .and_then(|answer| answer.ok())
                .and_then(|response| response.err());
            (finished, answer)
        };
        let refused = finished_with(Some(("cookie", b"session=1")));
        let empty = finished_with(None);
        assert_eq!(
            (refused, empty.0),
            (
                (
                    Err(ErrorCode::HTTPRequestDenied),
                    Some(ErrorCode::HTTPProtocolError)
                ),
                Ok(())
            ),
            "outgoing-body.finish with trailers holding a field must answer HTTP-request-denied \
             and fail its request, since an egress proxy sends no trailers and the request \
             policies never see them; trailers holding nothing must end the body"
        );
    }

    // @behavior NW-071
    #[test]
    fn a_body_dropped_unfinished_fails_its_request() {
        let mut guest = guest(Some(vec![
            network::Endpoint::new("example.com", 80).unwrap()
        ]));
        let request = greeting_request(&mut guest, "example.com").unwrap();
        let body = guest
            .types_outgoing_request_body(request)
            .unwrap()
            .expect("the body is handed out once");
        let request = guest.take_handle(request).unwrap();
        let future = guest
            .outgoing_handler_handle(request, None)
            .into_answer()
            .unwrap()
            .expect("the request waits on its body");
        let waiting = guest
            .types_future_incoming_response_get(future)
            .unwrap()
            .is_none();
        guest.types_outgoing_body_drop(body).unwrap();
        let answer = guest
            .types_future_incoming_response_get(future)
            .unwrap()
            .and_then(|answer| answer.ok())
            .and_then(|response| response.err());
        assert_eq!(
            (waiting, answer),
            (true, Some(ErrorCode::HTTPProtocolError)),
            "future-incoming-response.get must wait while the outgoing-body is open, and answer \
             HTTP-protocol-error once the body is dropped unfinished"
        );
    }
}
