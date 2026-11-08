//! What the Wasm Driver answers each `wasi:http` import a client makes. A
//! request the guest builds is handed to the Invocation's namespace, which
//! judges it on the guest thread and sends it through the outbound seam; until
//! a Grant gives the guest the network, `handle` answers `HTTP-request-denied`,
//! as a host that sends nothing does.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kobako_runtime::net::{self, Exchange, FieldError, HttpError, RequestBody, ResponseHead};

use super::bindings::wasi_http_types_0_2_12::{
    ErrorCode, Fields, FutureIncomingResponse, FutureTrailers, HeaderError, IncomingBody,
    IncomingResponse, Method, OutgoingBody, OutgoingRequest, RequestOptions, Scheme,
};
use super::bindings::wasi_io_poll_0_2_12::Pollable;
use super::bindings::wasi_io_streams_0_2_12::{InputStream, OutputStream};
use super::bindings::HttpHost;
use super::table::{self, Resource, Stream};
use crate::invocation::Invocation;

/// One header list, shared between a request and the `fields` its `headers`
/// hands out.
type Entries = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

fn locked(entries: &Entries) -> MutexGuard<'_, Vec<(String, Vec<u8>)>> {
    entries.lock().unwrap_or_else(PoisonError::into_inner)
}

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
    fn complete(&self) -> bool {
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
    headers: Entries,
    link: Arc<BodyLink>,
}

/// Where a `future-incoming-response` stands.
pub(crate) enum Future {
    /// Handed over before its body ended, so it leaves once it does.
    Waiting(Request),
    Sent(Arc<Exchange>),
    Refused(ErrorCode),
    /// Its response was taken.
    Taken,
}

/// One `wasi:http` resource the guest holds.
pub(crate) enum Held {
    Fields {
        entries: Entries,
        mutable: bool,
    },
    Request(Request),
    Body {
        link: Arc<BodyLink>,
        stream: bool,
    },
    Future(Future),
    Response {
        head: ResponseHead,
        exchange: Arc<Exchange>,
        consumed: bool,
    },
    IncomingBody {
        exchange: Arc<Exchange>,
        stream: bool,
    },
    Trailers,
}

impl Held {
    fn is_fields(&self) -> bool {
        matches!(self, Held::Fields { .. })
    }

    fn is_body(&self) -> bool {
        matches!(self, Held::Body { .. })
    }

    fn is_incoming_body(&self) -> bool {
        matches!(self, Held::IncomingBody { .. })
    }
}

/// The `header-error` a refused header answers.
fn to_header_error(err: FieldError) -> HeaderError {
    match err {
        FieldError::Forbidden => HeaderError::Forbidden,
        _ => HeaderError::InvalidSyntax,
    }
}

/// The error code a request's failure answers, carrying no host text.
fn to_error_code(err: HttpError) -> ErrorCode {
    match err {
        HttpError::Denied => ErrorCode::HTTPRequestDenied,
        HttpError::DestinationNotFound => ErrorCode::DestinationNotFound,
        HttpError::ConnectionRefused => ErrorCode::ConnectionRefused,
        HttpError::ConnectionTimeout => ErrorCode::ConnectionTimeout,
        HttpError::ConnectionTerminated => ErrorCode::ConnectionTerminated,
        HttpError::ProtocolError => ErrorCode::HTTPProtocolError,
        HttpError::ConnectionLimitReached => ErrorCode::ConnectionLimitReached,
        HttpError::TlsCertificate => ErrorCode::TLSCertificateError,
        HttpError::TlsProtocol => ErrorCode::TLSProtocolError,
        _ => ErrorCode::InternalError(None),
    }
}

fn method_name(method: &Method) -> Option<String> {
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
    fn held(&mut self, handle: u32) -> wasmtime::Result<&mut Held> {
        match self.wasi_mut().table_mut().get_mut(handle)? {
            Resource::Http(held) => Ok(held),
            _ => Err(wasmtime::Error::msg(
                "the guest named a handle that is not a wasi:http resource",
            )),
        }
    }

    /// Take `handle` out of the table as the `wasi:http` resource `kind`
    /// names; any other traps.
    fn take_held(&mut self, handle: u32, kind: fn(&Held) -> bool) -> wasmtime::Result<Held> {
        let taken = self
            .wasi_mut()
            .table_mut()
            .delete(handle, |r| matches!(r, Resource::Http(held) if kind(held)))?;
        match taken {
            Resource::Http(held) => Ok(held),
            _ => unreachable!("delete takes only what kind holds for"),
        }
    }

    fn push_http(&mut self, held: Held) -> wasmtime::Result<u32> {
        self.wasi_mut().table_mut().push(Resource::Http(held))
    }

    fn push_http_child(&mut self, held: Held, parent: u32) -> wasmtime::Result<u32> {
        self.wasi_mut()
            .table_mut()
            .push_child(Resource::Http(held), parent)
    }

    fn entries(&mut self, fields: Fields) -> wasmtime::Result<(Entries, bool)> {
        match self.held(fields.0)? {
            Held::Fields { entries, mutable } => Ok((entries.clone(), *mutable)),
            _ => Err(wasmtime::Error::msg(
                "the guest named a handle that is not fields",
            )),
        }
    }

    fn request(&mut self, request: OutgoingRequest) -> wasmtime::Result<&mut Request> {
        match self.held(request.0)? {
            Held::Request(request) => Ok(request),
            _ => Err(wasmtime::Error::msg(
                "the guest named a handle that is not an outgoing-request",
            )),
        }
    }

    /// Hand `request` to the namespace, which judges it here on the guest
    /// thread before the reactor sends it.
    fn send(&mut self, request: Request) -> Future {
        let Some(method) = method_name(&request.method) else {
            return Future::Refused(ErrorCode::HTTPRequestMethodInvalid);
        };
        let scheme = match request.scheme {
            Some(Scheme::HTTP) => net::Scheme::Http,
            Some(Scheme::HTTPS) | None => net::Scheme::Https,
            Some(Scheme::Other(_)) => return Future::Refused(ErrorCode::HTTPProtocolError),
        };
        let Some(authority) = request.authority else {
            return Future::Refused(ErrorCode::HTTPRequestURIInvalid);
        };
        let mut outgoing = net::OutgoingRequest::new(method, scheme, authority);
        if let Some(path) = request.path_with_query {
            outgoing.path_with_query = path;
        }
        outgoing.headers = locked(&request.headers).clone();
        outgoing.body = request.link.body.take();
        let Some(namespace) = self.wasi().table().network() else {
            return Future::Refused(ErrorCode::HTTPRequestDenied);
        };
        let cred = self.wasi().descriptors().cred();
        match namespace.send(cred, outgoing) {
            Ok(exchange) => Future::Sent(exchange),
            Err(err) => Future::Refused(to_error_code(err)),
        }
    }

    /// The future's request leaves once its body ended, and fails once its body
    /// was dropped unfinished.
    fn advance(&mut self, future: FutureIncomingResponse) -> wasmtime::Result<()> {
        if let Held::Future(state) = self.held(future.0)? {
            if matches!(state, Future::Waiting(request) if request.link.is_aborted()) {
                *state = Future::Refused(ErrorCode::HTTPProtocolError);
                return Ok(());
            }
        }
        let ready = matches!(
            self.held(future.0)?,
            Held::Future(Future::Waiting(request)) if request.link.complete()
        );
        if !ready {
            return Ok(());
        }
        let Held::Future(state) = self.held(future.0)? else {
            unreachable!("matched as a future above");
        };
        let Future::Waiting(request) = std::mem::replace(state, Future::Taken) else {
            unreachable!("matched as waiting above");
        };
        let sent = self.send(request);
        if let Held::Future(state) = self.held(future.0)? {
            *state = sent;
        }
        Ok(())
    }
}

impl HttpHost for Invocation {
    fn types_fields_constructor_new(&mut self) -> wasmtime::Result<Fields> {
        Ok(Fields(self.push_http(Held::Fields {
            entries: Entries::default(),
            mutable: true,
        })?))
    }

    fn types_fields_append(
        &mut self,
        this: Fields,
        name: String,
        value: Vec<u8>,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let (entries, mutable) = self.entries(this)?;
        if !mutable {
            return Ok(Err(HeaderError::Immutable));
        }
        match (
            net::field_name(&name).map_err(to_header_error),
            net::field_value(value).map_err(to_header_error),
        ) {
            (Ok(name), Ok(value)) => {
                locked(&entries).push((name, value));
                Ok(Ok(()))
            }
            (Err(err), _) | (_, Err(err)) => Ok(Err(err)),
        }
    }

    fn types_fields_entries(&mut self, this: Fields) -> wasmtime::Result<Vec<(String, Vec<u8>)>> {
        let (entries, _) = self.entries(this)?;
        let entries = locked(&entries).clone();
        Ok(entries)
    }

    fn types_fields_drop(&mut self, handle: Fields) -> wasmtime::Result<()> {
        self.take_held(handle.0, Held::is_fields).map(drop)
    }

    /// A GET with `headers`, which it takes, and a body the guest may ask for.
    fn types_outgoing_request_constructor_new(
        &mut self,
        headers: Fields,
    ) -> wasmtime::Result<OutgoingRequest> {
        let Held::Fields { entries, .. } = self.take_held(headers.0, Held::is_fields)? else {
            unreachable!("take_held takes only fields here")
        };
        let link = Arc::new(BodyLink {
            body: RequestBody::new(self.wasi().descriptors().cred().clone()),
            handed: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            aborted: AtomicBool::new(false),
        });
        Ok(OutgoingRequest(self.push_http(Held::Request(Request {
            method: Method::Get,
            scheme: None,
            authority: None,
            path_with_query: None,
            headers: entries,
            link,
        }))?))
    }

    /// The body, handed out once.
    fn types_outgoing_request_body(
        &mut self,
        this: OutgoingRequest,
    ) -> wasmtime::Result<Result<OutgoingBody, ()>> {
        let link = self.request(this)?.link.clone();
        if link.handed.swap(true, Ordering::AcqRel) {
            return Ok(Err(()));
        }
        Ok(Ok(OutgoingBody(self.push_http(Held::Body {
            link,
            stream: false,
        })?)))
    }

    fn types_outgoing_request_set_method(
        &mut self,
        this: OutgoingRequest,
        method: Method,
    ) -> wasmtime::Result<Result<(), ()>> {
        if method_name(&method).is_none() {
            return Ok(Err(()));
        }
        self.request(this)?.method = method;
        Ok(Ok(()))
    }

    fn types_outgoing_request_set_path_with_query(
        &mut self,
        this: OutgoingRequest,
        path_with_query: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        if path_with_query
            .as_deref()
            .is_some_and(|path| !net::is_origin_form(path))
        {
            return Ok(Err(()));
        }
        self.request(this)?.path_with_query = path_with_query;
        Ok(Ok(()))
    }

    fn types_outgoing_request_set_scheme(
        &mut self,
        this: OutgoingRequest,
        scheme: Option<Scheme>,
    ) -> wasmtime::Result<Result<(), ()>> {
        if matches!(scheme, Some(Scheme::Other(_))) {
            return Ok(Err(()));
        }
        self.request(this)?.scheme = scheme;
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
        self.request(this)?.authority = authority;
        Ok(Ok(()))
    }

    fn types_outgoing_request_drop(&mut self, handle: OutgoingRequest) -> wasmtime::Result<()> {
        self.take_held(handle.0, |held| matches!(held, Held::Request(_)))
            .map(drop)
    }

    /// The stream writing the body, handed out once.
    fn types_outgoing_body_write(
        &mut self,
        this: OutgoingBody,
    ) -> wasmtime::Result<Result<OutputStream, ()>> {
        let body = match self.held(this.0)? {
            Held::Body { link, stream } if !*stream => {
                *stream = true;
                link.body.clone()
            }
            Held::Body { .. } => return Ok(Err(())),
            _ => {
                return Err(wasmtime::Error::msg(
                    "the guest named a handle that is not an outgoing-body",
                ))
            }
        };
        Ok(Ok(OutputStream(self.wasi_mut().table_mut().push_child(
            Resource::OutputStream(Stream::Request(body)),
            this.0,
        )?)))
    }

    /// End the body, so its request may leave; HTTP/1 carries no trailers, so
    /// any given are dropped.
    fn types_outgoing_body_finish(
        &mut self,
        this: OutgoingBody,
        trailers: Option<Fields>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let Held::Body { link, .. } = self.take_held(this.0, Held::is_body)? else {
            unreachable!("take_held takes only an outgoing-body here")
        };
        if let Some(trailers) = trailers {
            self.take_held(trailers.0, Held::is_fields)?;
        }
        link.finished.store(true, Ordering::Release);
        Ok(Ok(()))
    }

    /// A body dropped unfinished fails its request, as wasmtime-wasi-http aborts
    /// it with an HTTP protocol error.
    fn types_outgoing_body_drop(&mut self, handle: OutgoingBody) -> wasmtime::Result<()> {
        if let Held::Body { link, .. } = self.take_held(handle.0, Held::is_body)? {
            link.aborted.store(true, Ordering::Release);
        }
        Ok(())
    }

    /// Ready once the response's head arrives; one whose request has not left
    /// yet, or that already answered, is ready at once.
    fn types_future_incoming_response_subscribe(
        &mut self,
        this: FutureIncomingResponse,
    ) -> wasmtime::Result<Pollable> {
        self.advance(this)?;
        let pollable = match self.held(this.0)? {
            Held::Future(Future::Sent(exchange)) => table::Pollable::Response(exchange.clone()),
            Held::Future(_) => table::Pollable::Ready,
            _ => {
                return Err(wasmtime::Error::msg(
                    "the guest named a handle that is not a future-incoming-response",
                ))
            }
        };
        Ok(Pollable(
            self.wasi_mut()
                .table_mut()
                .push(Resource::Pollable(pollable))?,
        ))
    }

    /// The response once its head arrives, handed out once.
    #[allow(clippy::type_complexity)]
    fn types_future_incoming_response_get(
        &mut self,
        this: FutureIncomingResponse,
    ) -> wasmtime::Result<Option<Result<Result<IncomingResponse, ErrorCode>, ()>>> {
        self.advance(this)?;
        let Held::Future(state) = self.held(this.0)? else {
            return Err(wasmtime::Error::msg(
                "the guest named a handle that is not a future-incoming-response",
            ));
        };
        let answer = match state {
            Future::Waiting(_) => return Ok(None),
            Future::Taken => return Ok(Some(Err(()))),
            Future::Refused(code) => Err(code.clone()),
            Future::Sent(exchange) => match exchange.response() {
                None => return Ok(None),
                Some(Ok(head)) => Ok((head, exchange.clone())),
                Some(Err(err)) => Err(to_error_code(err)),
            },
        };
        *state = Future::Taken;
        match answer {
            Ok((head, exchange)) => Ok(Some(Ok(Ok(IncomingResponse(self.push_http(
                Held::Response {
                    head,
                    exchange,
                    consumed: false,
                },
            )?))))),
            Err(code) => Ok(Some(Ok(Err(code)))),
        }
    }

    fn types_future_incoming_response_drop(
        &mut self,
        handle: FutureIncomingResponse,
    ) -> wasmtime::Result<()> {
        let future = self.take_held(handle.0, |held| matches!(held, Held::Future(_)))?;
        if let Held::Future(Future::Sent(exchange)) = future {
            exchange.release();
        }
        Ok(())
    }

    fn types_incoming_response_status(&mut self, this: IncomingResponse) -> wasmtime::Result<u16> {
        match self.held(this.0)? {
            Held::Response { head, .. } => Ok(head.status),
            _ => Err(wasmtime::Error::msg(
                "the guest named a handle that is not an incoming-response",
            )),
        }
    }

    /// The response's headers, which the guest cannot change.
    fn types_incoming_response_headers(
        &mut self,
        this: IncomingResponse,
    ) -> wasmtime::Result<Fields> {
        let entries = match self.held(this.0)? {
            Held::Response { head, .. } => head.headers.clone(),
            _ => {
                return Err(wasmtime::Error::msg(
                    "the guest named a handle that is not an incoming-response",
                ))
            }
        };
        Ok(Fields(self.push_http_child(
            Held::Fields {
                entries: Arc::new(Mutex::new(entries)),
                mutable: false,
            },
            this.0,
        )?))
    }

    /// The body, handed out once.
    fn types_incoming_response_consume(
        &mut self,
        this: IncomingResponse,
    ) -> wasmtime::Result<Result<IncomingBody, ()>> {
        let exchange = match self.held(this.0)? {
            Held::Response {
                exchange, consumed, ..
            } if !*consumed => {
                *consumed = true;
                exchange.clone()
            }
            Held::Response { .. } => return Ok(Err(())),
            _ => {
                return Err(wasmtime::Error::msg(
                    "the guest named a handle that is not an incoming-response",
                ))
            }
        };
        Ok(Ok(IncomingBody(self.push_http(Held::IncomingBody {
            exchange,
            stream: false,
        })?)))
    }

    /// A response dropped with its body never asked for lets the exchange go.
    fn types_incoming_response_drop(&mut self, handle: IncomingResponse) -> wasmtime::Result<()> {
        let response = self.take_held(handle.0, |held| matches!(held, Held::Response { .. }))?;
        if let Held::Response {
            exchange,
            consumed: false,
            ..
        } = response
        {
            exchange.release();
        }
        Ok(())
    }

    /// The stream reading the body, handed out once.
    fn types_incoming_body_stream(
        &mut self,
        this: IncomingBody,
    ) -> wasmtime::Result<Result<InputStream, ()>> {
        let exchange = match self.held(this.0)? {
            Held::IncomingBody { exchange, stream } if !*stream => {
                *stream = true;
                exchange.clone()
            }
            Held::IncomingBody { .. } => return Ok(Err(())),
            _ => {
                return Err(wasmtime::Error::msg(
                    "the guest named a handle that is not an incoming-body",
                ))
            }
        };
        Ok(Ok(InputStream(self.wasi_mut().table_mut().push_child(
            Resource::InputStream(Stream::Response(exchange)),
            this.0,
        )?)))
    }

    /// The trailers, which HTTP/1 never carries.
    fn types_incoming_body_finish(
        &mut self,
        this: IncomingBody,
    ) -> wasmtime::Result<FutureTrailers> {
        let Held::IncomingBody { exchange, .. } = self.take_held(this.0, Held::is_incoming_body)?
        else {
            unreachable!("take_held takes only an incoming-body here")
        };
        exchange.release();
        Ok(FutureTrailers(self.push_http(Held::Trailers)?))
    }

    fn types_incoming_body_drop(&mut self, handle: IncomingBody) -> wasmtime::Result<()> {
        self.take_held(handle.0, Held::is_incoming_body).map(drop)
    }

    fn types_future_trailers_drop(&mut self, handle: FutureTrailers) -> wasmtime::Result<()> {
        self.take_held(handle.0, |held| matches!(held, Held::Trailers))
            .map(drop)
    }

    /// Send `request`, which it takes. Without a network Grant it answers
    /// `HTTP-request-denied` at once, doing nothing first; a request whose body
    /// has not ended leaves once it does.
    fn outgoing_handler_handle(
        &mut self,
        request: OutgoingRequest,
        options: Option<RequestOptions>,
    ) -> wasmtime::Result<Result<FutureIncomingResponse, ErrorCode>> {
        let Held::Request(request) =
            self.take_held(request.0, |held| matches!(held, Held::Request(_)))?
        else {
            unreachable!("take_held takes only an outgoing-request here")
        };
        if options.is_some() {
            return Err(wasmtime::Error::msg(
                "the guest named request-options, which kobako never hands out",
            ));
        }
        if self.wasi().table().network().is_none() {
            return Ok(Err(ErrorCode::HTTPRequestDenied));
        }
        let future = match request.link.complete() {
            true => self.send(request),
            false => Future::Waiting(request),
        };
        if let Future::Refused(code) = future {
            return Ok(Err(code));
        }
        Ok(Ok(FutureIncomingResponse(
            self.push_http(Held::Future(future))?,
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::time::{Duration as Wait, Instant};

    use kobako_runtime::grant::Grant;
    use kobako_runtime::network;
    use kobako_runtime::profile::Profile;
    use kobako_runtime::sched::run_task;

    use super::*;
    use crate::config::Config;
    use crate::preview2::bindings::IoHost;
    use crate::wasi::{sandbox_connections, Wasi};

    /// A hermetic invocation whose network holds `endpoints`, or none at all.
    fn guest(endpoints: Option<Vec<network::Endpoint>>) -> Invocation {
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            open_connection_limit: None,
            connection_limit: None,
            profile: Profile::Hermetic,
            security: Vec::new(),
        };
        let grants = endpoints
            .map(|endpoints| {
                let mut entry = network::Builder::default();
                endpoints.into_iter().for_each(|endpoint| {
                    entry.endpoint(endpoint);
                });
                vec![Grant::Network(
                    network::Network::new("web", Arc::new(entry)).unwrap(),
                )]
            })
            .unwrap_or_default();
        let wasi = Wasi::for_invocation(&config, Vec::new(), &grants, &sandbox_connections())
            .expect("WASI context must build");
        Invocation::new(None, wasi)
    }

    /// A GET for `/greeting` at `authority` over HTTP.
    fn get(guest: &mut Invocation, authority: &str) -> wasmtime::Result<OutgoingRequest> {
        let headers = guest.types_fields_constructor_new()?;
        let request = guest.types_outgoing_request_constructor_new(headers)?;
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
        let request = get(&mut guest, "example.com").unwrap();
        assert!(
            matches!(
                guest.outgoing_handler_handle(request, None),
                Ok(Err(ErrorCode::HTTPRequestDenied))
            ),
            "outgoing-handler.handle with no network entry must answer HTTP-request-denied"
        );
    }

    // @behavior NW-040
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
        let (status, kind, body) = run_task(Some(started + Wait::from_secs(10)), || {
            let request = get(&mut guest, &format!("127.0.0.1:{port}"))?;
            let future = guest
                .outgoing_handler_handle(request, None)?
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
            while let Ok(bytes) = guest.streams_input_stream_blocking_read(stream, 64)? {
                read.extend(bytes);
            }
            Ok::<_, wasmtime::Error>((status, kind, read))
        })
        .expect("the request answers");
        assert_eq!(
            (status, kind, body, server.join().unwrap()),
            (
                201,
                vec![b"test".to_vec()],
                b"hello".to_vec(),
                "GET /greeting HTTP/1.1\r\n".to_string()
            ),
            "a GET built through wasi:http to an http endpoint must reach the server and read \
             back the status, a header under its lowercase name, and the body to its end"
        );
    }

    // @behavior NW-041
    #[test]
    fn a_forbidden_header_or_a_response_header_cannot_be_set() {
        let mut guest = guest(None);
        let fields = guest.types_fields_constructor_new().unwrap();
        let set = |guest: &mut Invocation, fields, name: &str| {
            guest
                .types_fields_append(fields, name.to_string(), b"x".to_vec())
                .unwrap()
        };
        let answers = [
            set(&mut guest, fields, "Host"),
            set(&mut guest, fields, "content-length"),
            set(&mut guest, fields, "bad name"),
            set(&mut guest, fields, "x-ok"),
        ];
        let frozen = guest.push_http(Held::Fields {
            entries: Entries::default(),
            mutable: false,
        });
        let immutable = set(&mut guest, Fields(frozen.unwrap()), "x-ok");
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

    // @behavior NW-064
    #[test]
    fn a_body_dropped_or_finished_while_its_stream_lives_traps() {
        let mut guest = guest(None);
        let request = get(&mut guest, "example.com").unwrap();
        let body = guest
            .types_outgoing_request_body(request)
            .unwrap()
            .expect("the body is handed out once");
        let stream = guest
            .types_outgoing_body_write(body)
            .unwrap()
            .expect("the stream is handed out once");
        let dropped = guest.types_outgoing_body_drop(body).is_err();
        let finished = guest.types_outgoing_body_finish(body, None).is_err();
        guest.streams_output_stream_drop(stream).unwrap();
        let after = guest.types_outgoing_body_finish(body, None).unwrap();
        assert_eq!(
            (dropped, finished, after),
            (true, true, Ok(())),
            "outgoing-body.drop and finish must trap while the body's output-stream lives, \
             and finish once the stream is dropped"
        );
    }

    // @behavior NW-071
    #[test]
    fn a_body_dropped_unfinished_fails_its_request() {
        let mut guest = guest(Some(vec![
            network::Endpoint::new("example.com", 80).unwrap()
        ]));
        let request = get(&mut guest, "example.com").unwrap();
        let body = guest
            .types_outgoing_request_body(request)
            .unwrap()
            .expect("the body is handed out once");
        let future = guest
            .outgoing_handler_handle(request, None)
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
