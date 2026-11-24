//! What the Wasm Driver answers each `wasi:http` import a client makes. A
//! request the guest builds is handed to the Invocation's namespace, which
//! judges it on the guest thread and sends it through the outbound seam; until
//! a Grant gives the guest the network, `handle` answers `HTTP-request-denied`,
//! as a host that sends nothing does.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kobako_runtime::net::{self, Exchange, FieldError, RequestBody};

use super::bindings::wasi_http_types_0_2_12::{
    DNSErrorPayload, ErrorCode, Fields, FutureIncomingResponse, FutureTrailers, HeaderError,
    IncomingBody, IncomingResponse, Method, OutgoingBody, OutgoingRequest, RequestOptions, Scheme,
    TLSAlertReceivedPayload,
};
use super::bindings::wasi_io_poll_0_2_12::Pollable;
use super::bindings::wasi_io_streams_0_2_12::{InputStream, OutputStream};
use super::bindings::HttpHost;
use super::error;
use super::io::{self, SharedFile, Stream};
use super::table;
use super::to_nanos;
use crate::invocation::Invocation;

/// A request's body and whether the guest asked for it, shared between the
/// request and the `outgoing-body` its `body` hands out.
pub(crate) struct BodyLink {
    body: Arc<RequestBody>,
    handed: AtomicBool,
}

/// A request the guest is building.
pub(crate) struct Request {
    method: Method,
    scheme: Option<Scheme>,
    authority: Option<String>,
    path_with_query: Option<String>,
    headers: Arc<net::Fields>,
    link: Arc<BodyLink>,
    /// The options `handle` was given.
    timeouts: net::Timeouts,
}

impl Request {
    /// The room its method, authority, path, and headers take together, as
    /// a server's header buffer holds the request line beside the headers.
    fn room(&self) -> usize {
        let method = match &self.method {
            Method::Other(other) => other.len(),
            _ => 0,
        };
        method
            + self.authority.as_ref().map_or(0, String::len)
            + self.path_with_query.as_ref().map_or(0, String::len)
            + self.headers.room()
    }
}

/// Where a `future-incoming-response` stands.
pub(crate) enum FutureState {
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

    /// A response whose head its exchange holds once.
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

/// The error code a request's failure answers, carrying no host text.
fn to_error_code(err: net::HttpError) -> ErrorCode {
    match err {
        net::HttpError::Denied => ErrorCode::HTTPRequestDenied,
        // wasmtime-wasi-http answers a lookup that failed this way.
        net::HttpError::DnsError => ErrorCode::DNSError(DNSErrorPayload {
            rcode: Some("address not available".to_string()),
            info_code: None,
        }),
        net::HttpError::DestinationUnavailable => ErrorCode::DestinationUnavailable,
        net::HttpError::ConnectionRefused => ErrorCode::ConnectionRefused,
        net::HttpError::ConnectionTimeout => ErrorCode::ConnectionTimeout,
        net::HttpError::ConnectionReadTimeout => ErrorCode::ConnectionReadTimeout,
        net::HttpError::ConnectionTerminated => ErrorCode::ConnectionTerminated,
        net::HttpError::ProtocolError => ErrorCode::HTTPProtocolError,
        net::HttpError::ConnectionLimitReached => ErrorCode::ConnectionLimitReached,
        net::HttpError::TlsCertificate => ErrorCode::TLSCertificateError,
        net::HttpError::TlsAlertReceived(alert_id) => {
            ErrorCode::TLSAlertReceived(TLSAlertReceivedPayload {
                alert_id: Some(alert_id),
                alert_message: None,
            })
        }
        net::HttpError::TlsProtocol => ErrorCode::TLSProtocolError,
        _ => ErrorCode::InternalError(None),
    }
}

fn name_of(method: &Method) -> &str {
    match method {
        Method::Get => "GET",
        Method::Head => "HEAD",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
        Method::Connect => "CONNECT",
        Method::Options => "OPTIONS",
        Method::Trace => "TRACE",
        Method::Patch => "PATCH",
        Method::Other(other) => other,
    }
}

impl Invocation {
    fn request_mut(&mut self, request: OutgoingRequest) -> wasmtime::Result<&mut Request> {
        self.table_mut()
            .get_mut(request)
            .map(|request| &mut request.0)
    }

    /// Change `request` through `change`; past `MAX_FIELD_SIZE` for its
    /// parts and headers together the call traps, as a header list's append
    /// does.
    fn change_request(
        &mut self,
        request: OutgoingRequest,
        change: impl FnOnce(&mut Request),
    ) -> wasmtime::Result<()> {
        let request = self.request_mut(request)?;
        change(request);
        match request.room() > net::MAX_FIELD_SIZE {
            true => Err(wasmtime::Error::msg(
                "the guest's request outgrows the most one request's head may hold",
            )),
            false => Ok(()),
        }
    }

    /// Room for a header list or a request the host holds for the guest, which
    /// takes an open file as a host-side producer does; none left traps,
    /// since no constructor has an error for it.
    fn reserve_held(&self) -> wasmtime::Result<table::FileRoom> {
        self.wasi().table().reserve_producer().ok_or_else(|| {
            wasmtime::Error::msg("the guest's open files leave no room for what the host holds")
        })
    }

    fn future_mut(&mut self, future: FutureIncomingResponse) -> wasmtime::Result<&mut FutureState> {
        self.table_mut().get_mut(future).map(|future| &mut future.0)
    }

    /// Hand `request` to the namespace, which judges it here on the guest
    /// thread before it is sent. Its body streams as the guest writes it; one
    /// the guest never asked for is empty.
    fn send(&mut self, request: Request) -> FutureState {
        if !request.link.handed.load(Ordering::Acquire) {
            request.link.body.finish();
        }
        let method = name_of(&request.method).to_string();
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
        outgoing.body = request.link.body.clone();
        outgoing.timeouts = request.timeouts;
        let cred = self.wasi().cred();
        match net::send(self.network(), cred, outgoing) {
            Ok(exchange) => {
                let file = Arc::new(Mutex::new(exchange.alloc_response_file(cred)));
                FutureState::Sent(exchange, file)
            }
            Err(err) => FutureState::Refused(to_error_code(err)),
        }
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
        let room = self.reserve_held()?;
        Ok(self.table_mut().install(
            room,
            resource::Fields {
                entries: net::Fields::new(),
                mutable: true,
            },
        ))
    }

    /// A header appended, or the `header-error` refusing it; a list past
    /// `MAX_FIELD_SIZE` traps, as wasmtime's field limit does.
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
            Err(FieldError::TooLarge) => Err(wasmtime::Error::msg(
                "the guest's headers outgrow the most one list may hold",
            )
            .into()),
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
        // The list's own place went with its handle, so the request takes it.
        let room = self.reserve_held()?;
        let link = Arc::new(BodyLink {
            body: RequestBody::new(),
            handed: AtomicBool::new(false),
        });
        let request = Request {
            method: Method::Get,
            scheme: None,
            authority: None,
            path_with_query: None,
            headers: headers.entries,
            link,
            timeouts: net::Timeouts::default(),
        };
        Ok(self
            .table_mut()
            .install(room, resource::OutgoingRequest(request)))
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
        if !net::is_token(name_of(&method)) {
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
        let link = body.link.clone();
        let file = link.body.alloc_file(self.wasi().cred());
        let stream = Stream::RequestBody(link.body.clone(), Arc::new(Mutex::new(file)));
        self.table_mut()
            .push_child(io::OutputStream::new(stream), this)
            .map(Ok)
    }

    /// End the body, so its request ends once the seam sends what is left.
    /// Trailers holding a field answer `HTTP-request-denied` and fail the
    /// request: an egress proxy sends none by default, as Envoy drops HTTP/1
    /// trailers, and the request policies never see them, so dropping them
    /// would succeed differently.
    fn types_outgoing_body_finish(
        &mut self,
        this: resource::OutgoingBody,
        trailers: Option<resource::Fields>,
    ) -> Result<(), error::HttpError> {
        if trailers.is_some_and(|trailers| !trailers.entries.entries().is_empty()) {
            this.link.body.fail(net::HttpError::ProtocolError);
            return Err(ErrorCode::HTTPRequestDenied.into());
        }
        this.link.body.finish();
        Ok(())
    }

    /// A body dropped unfinished fails its request, as wasmtime-wasi-http aborts
    /// it with an HTTP protocol error.
    fn types_outgoing_body_drop(&mut self, handle: OutgoingBody) -> wasmtime::Result<()> {
        let link = self.table_mut().delete(handle)?.link;
        link.body.fail(net::HttpError::ProtocolError);
        Ok(())
    }

    /// Ready once the response's head arrives; one already answered is ready
    /// at once.
    fn types_future_incoming_response_subscribe(
        &mut self,
        this: FutureIncomingResponse,
    ) -> wasmtime::Result<Pollable> {
        let pollable = match self.future_mut(this)? {
            FutureState::Sent(exchange, file) => {
                io::Pollable::Exchange(file.clone(), exchange.clone())
            }
            _ => io::Pollable::Ready,
        };
        self.table_mut().push_child(pollable, this)
    }

    /// The response once its head arrives, handed out once.
    #[allow(clippy::type_complexity)]
    fn types_future_incoming_response_get(
        &mut self,
        this: FutureIncomingResponse,
    ) -> wasmtime::Result<Option<Result<Result<IncomingResponse, ErrorCode>, ()>>> {
        let state = self.future_mut(this)?;
        let answer = match state {
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
        let room = self.reserve_held()?;
        Ok(self.table_mut().install(
            room,
            resource::Fields {
                entries: net::Fields::with_entries(headers),
                mutable: false,
            },
        ))
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
                io::InputStream::for_body(Stream::File(file), exchange),
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
        Ok(self.table_mut().get(this)?.0.connect.map(to_nanos))
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
        Ok(self.table_mut().get(this)?.0.first_byte.map(to_nanos))
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
        Ok(self.table_mut().get(this)?.0.between_bytes.map(to_nanos))
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
    /// `HTTP-request-denied` at once, doing nothing first. The request leaves
    /// at once, bound by the timeouts `options` sets, and its body streams
    /// after it as the guest writes it, as wasi-http's backpressure asks.
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
        let Some(room) = self.wasi().table().reserve_producer() else {
            return Err(ErrorCode::ConnectionLimitReached.into());
        };
        let future = self.send(request);
        if let FutureState::Refused(code) = future {
            return Err(code.into());
        }
        Ok(self
            .table_mut()
            .install(room, resource::FutureIncomingResponse(future)))
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
    use crate::preview2::bindings::{ClocksHost, Host as _, IoHost};

    // @behavior NW-225
    #[test]
    fn a_lookup_that_failed_answers_dns_error_as_wasmtime_wasi_http_does() {
        assert!(
            matches!(
                to_error_code(net::HttpError::DnsError),
                ErrorCode::DNSError(DNSErrorPayload { rcode: Some(rcode), info_code: None })
                    if rcode == "address not available"
            ),
            "a request whose name has no address must answer DNS-error with the rcode \
             wasmtime-wasi-http gives a failed lookup"
        );
    }

    // @behavior NW-225
    #[test]
    fn a_tls_alert_answers_tls_alert_received_with_its_id() {
        assert!(
            matches!(
                to_error_code(net::HttpError::TlsAlertReceived(40)),
                ErrorCode::TLSAlertReceived(TLSAlertReceivedPayload {
                    alert_id: Some(40),
                    alert_message: None
                })
            ),
            "a request whose server sent the handshake_failure alert must answer \
             TLS-alert-received carrying alert id 40 and no message, as wasmtime-wasi-http does"
        );
    }

    /// A hermetic invocation whose network holds `endpoints`, or none at all.
    fn guest(endpoints: Option<Vec<network::Endpoint>>) -> Invocation {
        limited_guest(endpoints, None)
    }

    /// A guest as `guest` makes one, allotted `memory_limit` bytes.
    fn limited_guest(
        endpoints: Option<Vec<network::Endpoint>>,
        memory_limit: Option<usize>,
    ) -> Invocation {
        Invocation::hermetic(endpoints, |config| {
            config.limits.memory_limit = memory_limit
        })
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

    /// A guest holding the header list `incoming-response.headers` hands out
    /// for a head a loopback server answers.
    fn guest_with_response_headers() -> (Invocation, Fields) {
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
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nx-kind: test\r\n\r\n")
                .unwrap();
        });
        let endpoint = network::Endpoint::new("127.0.0.1", port).unwrap();
        let mut guest = guest(Some(vec![endpoint]));
        let started = Instant::now();
        let headers = run_task(Some(started + Wait::from_secs(10)), || {
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
            guest.types_incoming_response_headers(response)
        })
        .expect("a loopback server's head answers; if not, look at the request before the headers");
        server.join().unwrap();
        (guest, headers)
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
            let unset = [
                guest.types_request_options_connect_timeout(options)?,
                guest.types_request_options_first_byte_timeout(options)?,
                guest.types_request_options_between_bytes_timeout(options)?,
            ];
            let set = [
                guest.types_request_options_set_connect_timeout(options, Some(7))?,
                guest.types_request_options_set_first_byte_timeout(options, Some(0))?,
                guest.types_request_options_set_between_bytes_timeout(options, Some(9))?,
            ];
            let read_back = (
                unset,
                set,
                [
                    guest.types_request_options_connect_timeout(options)?,
                    guest.types_request_options_first_byte_timeout(options)?,
                    guest.types_request_options_between_bytes_timeout(options)?,
                ],
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
                (
                    [None, None, None],
                    [Ok(()), Ok(()), Ok(())],
                    [Some(7), Some(0), Some(9)]
                ),
                Some(ErrorCode::ConnectionReadTimeout),
                true
            ),
            "request-options must read back each of its three timeouts as set, and none of them \
             until set, and a request handled with a first-byte timeout must wake its future's \
             pollable once the timeout passes, not at the run's deadline, and answer \
             connection-read-timeout"
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
                    b"HTTP/1.1 201 Created\r\ncontent-length: 5\r\nX-Kind: test\r\n\r\nhello",
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
             back the status, a header the server sent as X-Kind under its lowercase name, and \
             the body to its end; a read of nothing past the end must answer closed, as io.wit \
             has it"
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
        let (mut held, response_headers) = guest_with_response_headers();
        let immutable = set(&mut held, response_headers, "x-ok");
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
             token, and any change to the list incoming-response.headers hands out"
        );
    }

    // @behavior NW-081
    #[test]
    fn a_header_list_past_its_room_traps() {
        let mut guest = limited_guest(None, Some(16 * 1024));
        let fields = guest.types_fields_new().unwrap();
        let mut taken = 0;
        let trap = loop {
            if let Err(trap) = guest
                .types_fields_append(fields, format!("x-{taken}"), vec![b'v'; 1024])
                .into_answer()
            {
                break trap.to_string();
            }
            taken += 1;
        };
        assert_eq!(
            (trap.as_str(), taken > 16 && taken < 128),
            (
                "the guest's headers outgrow the most one list may hold",
                true
            ),
            "fields.append of 1 KiB values under a 16 KiB memory_limit must trap once the list \
             passes 128 KiB, whatever the memory_limit; took {taken}"
        );
    }

    // @behavior NW-156
    #[test]
    fn a_requests_line_shares_its_headers_room() {
        // Whether a request whose headers hold `headers` bytes takes a path of
        // `path` bytes.
        let set = |headers: usize, path: usize| {
            let mut guest = guest(None);
            let fields = guest.types_fields_new().unwrap();
            guest
                .types_fields_append(fields, "x-pad".to_string(), vec![b'v'; headers])
                .into_answer()
                .unwrap()
                .expect("a header within the list's room appends");
            let fields = guest.take_handle(fields).unwrap();
            let request = guest.types_outgoing_request_new(fields).unwrap();
            let path = format!("/{}", "p".repeat(path - 1));
            guest
                .types_outgoing_request_set_path_with_query(request, Some(path))
                .map(drop)
                .map_err(|trap| trap.to_string())
        };
        assert_eq!(
            (set(96 * 1024, 16 * 1024), set(96 * 1024, 40 * 1024)),
            (
                Ok(()),
                Err(
                    "the guest's request outgrows the most one request's head may hold".to_string()
                )
            ),
            "outgoing-request.set-path-with-query must take a 16 KiB path beside 96 KiB of \
             headers, and trap on a 40 KiB one, which passes the 128 KiB they share"
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
        let (mut guest, response_headers) = guest_with_response_headers();
        assert_eq!(
            guest
                .types_fields_append(response_headers, "bad name".to_string(), b"x".to_vec())
                .into_answer()
                .unwrap(),
            Err(HeaderError::InvalidSyntax),
            "fields.append of a name that is no token to the list incoming-response.headers \
             hands out must answer invalid-syntax, as wasmtime reads the header before it finds \
             the list immutable"
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

    /// A loopback server that takes connections and never answers, and the
    /// guest whose one http endpoint names it.
    fn guest_of_a_silent_server() -> (Invocation, String, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let guest = guest(Some(vec![
            network::Endpoint::new("127.0.0.1", port).unwrap()
        ]));
        (guest, format!("127.0.0.1:{port}"), listener)
    }

    /// The error `future` answers once its request settles.
    fn settled_error(guest: &mut Invocation, future: FutureIncomingResponse) -> Option<ErrorCode> {
        run_task(Some(Instant::now() + Wait::from_secs(10)), || {
            let ready = guest.types_future_incoming_response_subscribe(future)?;
            guest.poll_pollable_block(ready)?;
            guest.poll_pollable_drop(ready)?;
            Ok::<_, wasmtime::Error>(
                guest
                    .types_future_incoming_response_get(future)?
                    .and_then(|answer| answer.ok())
                    .and_then(|response| response.err()),
            )
        })
        .expect("the request settles")
    }

    /// The body a chunked request carries, read from `reader` to its last
    /// chunk.
    fn read_chunked(reader: &mut impl BufRead) -> Vec<u8> {
        let mut body = Vec::new();
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).unwrap();
            let size = usize::from_str_radix(size.trim(), 16).unwrap();
            let mut chunk = vec![0; size + 2];
            std::io::Read::read_exact(reader, &mut chunk).unwrap();
            if size == 0 {
                return body;
            }
            body.extend_from_slice(&chunk[..size]);
        }
    }

    // @behavior NW-251
    #[test]
    fn a_request_body_holds_one_socket_buffer_until_the_server_takes_it() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (start_reading, reading) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            reading.recv().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while line != "\r\n" {
                line.clear();
                reader.read_line(&mut line).unwrap();
            }
            let body = read_chunked(&mut reader);
            (&stream)
                .write_all(b"HTTP/1.1 204 No Content\r\n\r\n")
                .unwrap();
            body
        });
        let mut guest = guest(Some(vec![
            network::Endpoint::new("127.0.0.1", port).unwrap()
        ]));
        let sent = (0..8 << 20).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        let deadline = Instant::now() + Wait::from_secs(20);
        let (most, unready, status) = run_task(Some(deadline), || {
            let request = greeting_request(&mut guest, &format!("127.0.0.1:{port}"))?;
            guest
                .types_outgoing_request_set_method(request, Method::Post)?
                .expect("POST is a method");
            let body = guest
                .types_outgoing_request_body(request)?
                .expect("the body is handed out once");
            let output = guest
                .types_outgoing_body_write(body)?
                .expect("the stream is handed out once");
            let request = guest.take_handle(request)?;
            let future = guest
                .outgoing_handler_handle(request, None)
                .into_answer()?
                .expect("an http endpoint names the authority");
            let writable = guest.streams_output_stream_subscribe(output)?;
            let (mut written, mut most, mut unready) = (0, 0, false);
            while written < sent.len() {
                let permit = guest
                    .streams_output_stream_check_write(output)
                    .into_answer()?
                    .expect("an open stream permits") as usize;
                most = most.max(permit);
                if permit == 0 {
                    // The seam may still move what is queued into the
                    // host's socket; once that is full too, nothing frees room.
                    if !unready {
                        let now = guest.monotonic_clock_subscribe_duration(0)?;
                        unready = !guest.poll_poll(vec![writable, now])?.contains(&0);
                        guest.poll_pollable_drop(now)?;
                        if !unready {
                            continue;
                        }
                        start_reading.send(()).unwrap();
                    }
                    guest.poll_pollable_block(writable)?;
                    continue;
                }
                let take = permit.min(sent.len() - written);
                guest
                    .streams_output_stream_write(output, sent[written..written + take].to_vec())
                    .into_answer()?
                    .expect("a write of the permit succeeds");
                written += take;
            }
            if !unready {
                start_reading.send(()).unwrap();
            }
            guest.poll_pollable_drop(writable)?;
            guest.streams_output_stream_drop(output)?;
            let body = guest.take_handle(body)?;
            guest
                .types_outgoing_body_finish(body, None)
                .into_answer()?
                .expect("the body ends");
            let ready = guest.types_future_incoming_response_subscribe(future)?;
            guest.poll_pollable_block(ready)?;
            let response = guest
                .types_future_incoming_response_get(future)?
                .expect("the head arrived")
                .expect("handed out once")
                .expect("the server answered");
            Ok::<_, wasmtime::Error>((
                most,
                unready,
                guest.types_incoming_response_status(response)?,
            ))
        })
        .expect("the request answers");
        let received = server.join().unwrap();
        assert!(
            most <= 64 * 1024 && unready && status == 204 && received == sent,
            "an 8 MiB request body to a server that reads it late must be permitted at \
             most one 64 KiB socket buffer at a time, poll unready while the buffer is full, and \
             reach the server whole and in order; permitted at most {most}, unready {unready}, \
             status {status}, the server read {} bytes",
            received.len()
        );
    }

    // @behavior NW-193
    #[test]
    fn a_body_finished_with_trailers_is_refused_and_fails_its_request() {
        let (mut guest, authority, _server) = guest_of_a_silent_server();
        // Whether `finish` with `trailers` refuses, and what the request's
        // future answers then.
        let finished_with = |guest: &mut Invocation, trailer: Option<(&str, &[u8])>| {
            let request = greeting_request(guest, &authority).unwrap();
            let body = guest
                .types_outgoing_request_body(request)
                .unwrap()
                .expect("the body is handed out once");
            let request = guest.take_handle(request).unwrap();
            let future = guest
                .outgoing_handler_handle(request, None)
                .into_answer()
                .unwrap()
                .expect("an http endpoint names the authority");
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
            (finished, future)
        };
        let (refused, refused_future) = finished_with(&mut guest, Some(("cookie", b"session=1")));
        let refused = (refused, settled_error(&mut guest, refused_future));
        let empty = finished_with(&mut guest, None);
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
        let (mut guest, authority, _server) = guest_of_a_silent_server();
        let request = greeting_request(&mut guest, &authority).unwrap();
        let body = guest
            .types_outgoing_request_body(request)
            .unwrap()
            .expect("the body is handed out once");
        let request = guest.take_handle(request).unwrap();
        let future = guest
            .outgoing_handler_handle(request, None)
            .into_answer()
            .unwrap()
            .expect("an http endpoint names the authority");
        let waiting = guest
            .types_future_incoming_response_get(future)
            .unwrap()
            .is_none();
        guest.types_outgoing_body_drop(body).unwrap();
        let answer = settled_error(&mut guest, future);
        assert_eq!(
            (waiting, answer),
            (true, Some(ErrorCode::HTTPProtocolError)),
            "future-incoming-response.get must wait while the outgoing-body is open, and answer \
             HTTP-protocol-error once the body is dropped unfinished"
        );
    }
}
