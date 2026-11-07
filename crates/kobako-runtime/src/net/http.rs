//! An HTTP request the guest sends, as a userspace proxy forwards one: the
//! namespace judges what the guest named on the guest thread, then a worker
//! hands the request to the outbound seam, whose only dialer is kobako's
//! connector. The response's body fills a buffer the guest reads through the
//! wait queue, so a full buffer stops the reader and the TCP window pushes
//! back.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::workers::{charge, Workers};
use super::{ProtocolFamily, Stream, SOCKET_BUFFER};
use crate::network::{Protocol, Ruleset};
use crate::vfs::{Cred, FilesystemError, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// Whether a request travels in clear or over TLS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Scheme {
    Http,
    Https,
}

/// One request the guest sends: its method, scheme, authority, path with its
/// query, headers, and body.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutgoingRequest {
    pub method: String,
    pub scheme: Scheme,
    pub authority: String,
    pub path_with_query: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

impl OutgoingRequest {
    pub fn new(method: impl Into<String>, scheme: Scheme, authority: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            scheme,
            authority: authority.into(),
            path_with_query: "/".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
}

/// The body a guest writes for a request, its bytes held on the guest's behalf
/// against the Invocation's memory, as `vm_committed_as` counts a mapping, so a
/// guest cannot grow the host past what it was allotted.
pub struct RequestBody {
    cred: Arc<Cred>,
    bytes: Mutex<Vec<u8>>,
}

impl RequestBody {
    /// An empty body, its bytes charged to `cred`.
    pub fn new(cred: Arc<Cred>) -> Arc<Self> {
        Arc::new(Self {
            cred,
            bytes: Mutex::new(Vec::new()),
        })
    }

    /// Append `buf`, refused past the Invocation's memory as `ENOMEM`.
    pub fn write(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        self.cred.security_vm_enough_memory(buf.len())?;
        self.bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    /// Take what was written, given back to the Invocation's memory, as the
    /// request leaves with it.
    pub fn take(&self) -> Vec<u8> {
        let bytes = std::mem::take(&mut *self.bytes.lock().unwrap_or_else(PoisonError::into_inner));
        self.cred.vm_unacct_memory(bytes.len());
        bytes
    }
}

impl Drop for RequestBody {
    fn drop(&mut self) {
        let held = self
            .bytes
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        self.cred.vm_unacct_memory(held);
    }
}

/// Why a request has no response, as WASI's `wasi:http` error codes name each
/// a client may meet; none carries host text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HttpError {
    /// No entry lets the guest send it.
    Denied,
    /// The authority's name resolves to nothing.
    DestinationNotFound,
    /// The authority refused the connection.
    ConnectionRefused,
    /// The connection did not open in time.
    ConnectionTimeout,
    /// The connection ended before the response did.
    ConnectionTerminated,
    /// The response was not one HTTP/1 can read.
    ProtocolError,
    /// The request cannot start now, as a full worker cap leaves it.
    ConnectionLimitReached,
    /// The seam failed in a way no other code names.
    Internal,
}

/// A response's head: its status and headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: u16,
    pub headers: Vec<(String, Vec<u8>)>,
}

/// What the outbound seam answers: the head, and the body to read.
pub struct Response {
    pub head: ResponseHead,
    pub body: Box<dyn Read + Send>,
}

/// The replaceable HTTP sender, as wasmtime's `send_request`. It dials only
/// through the connector it is handed; one dialing any other way skips every
/// rule, which is its builder's own choice.
pub trait Outbound: Send + Sync {
    /// Send `request` through `connector`, giving up once `timeout` passes.
    fn send_request(
        &self,
        request: &OutgoingRequest,
        connector: &Connector,
        timeout: Option<Duration>,
    ) -> Result<Response, HttpError>;
}

/// The only dialer the outbound seam has: it reaches the one authority the
/// request named, resolving a name on the host and dialing each address the
/// ruleset lets it lead to, exactly, in turn.
pub struct Connector {
    host: String,
    port: u16,
    ruleset: Arc<Ruleset>,
    family: Arc<dyn ProtocolFamily>,
}

impl Connector {
    /// The host and port the request named.
    pub fn authority(&self) -> (&str, u16) {
        (&self.host, self.port)
    }

    /// A stream to the authority, given up once `timeout` passes.
    pub fn dial(&self, timeout: Option<Duration>) -> Result<Box<dyn Stream>, HttpError> {
        let addrs = match self.host.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, self.port)],
            Err(_) => {
                let local = self.family.local_addresses();
                let real = self
                    .family
                    .resolve(&self.host)
                    .map_err(|_| HttpError::DestinationNotFound)?;
                if real.is_empty() {
                    return Err(HttpError::DestinationNotFound);
                }
                real.into_iter()
                    .map(|ip| SocketAddr::new(ip, self.port))
                    .filter(|addr| {
                        self.ruleset
                            .admit_resolved(&self.host, *addr, Protocol::Http, &local)
                            .is_ok()
                    })
                    .collect()
            }
        };
        if addrs.is_empty() {
            return Err(HttpError::Denied);
        }
        let mut last = HttpError::ConnectionRefused;
        for addr in addrs {
            match self.family.connect(addr, timeout) {
                Ok(stream) => return Ok(stream),
                Err(err) if err.kind() == io::ErrorKind::TimedOut => {
                    last = HttpError::ConnectionTimeout
                }
                Err(_) => last = HttpError::ConnectionRefused,
            }
        }
        Err(last)
    }
}

/// The host and port an authority names, the port defaulting by `scheme`;
/// `None` for one no URI holds.
pub(crate) fn split_authority(authority: &str, scheme: Scheme) -> Option<(String, u16)> {
    let default = match scheme {
        Scheme::Http => 80,
        Scheme::Https => 443,
    };
    if authority.is_empty() || authority.contains(['@', '/', ' ']) {
        return None;
    }
    // A bare IPv6 address holds colons with no port after them.
    let (host, port) = match authority.rsplit_once(':') {
        Some((head, _)) if !head.ends_with(']') && head.contains(':') => (authority, None),
        Some((head, port)) => (head, Some(port.parse::<u16>().ok()?)),
        None => (authority, None),
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    (!host.is_empty()).then(|| (host.to_ascii_lowercase(), port.unwrap_or(default)))
}

impl Connector {
    pub(crate) fn new(
        host: String,
        port: u16,
        ruleset: Arc<Ruleset>,
        family: Arc<dyn ProtocolFamily>,
    ) -> Self {
        Self {
            host,
            port,
            ruleset,
            family,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Sending,
    Answered(ResponseHead),
    Failed(HttpError),
}

struct State {
    phase: Phase,
    body: VecDeque<u8>,
    body_end: bool,
    body_error: bool,
    released: bool,
}

/// One request in flight, then its response: the head once it arrives, and
/// the body as a worker reads it.
pub struct Exchange {
    state: Mutex<State>,
    reader: Condvar,
    wait: Arc<WaitQueueHead>,
}

impl Exchange {
    /// Send `request` through `seam` on a worker counted against `workers`,
    /// dialing through `connector`, giving up at `deadline`.
    pub(crate) fn start(
        seam: Arc<dyn Outbound>,
        request: OutgoingRequest,
        connector: Connector,
        deadline: Option<Instant>,
        workers: &Arc<Workers>,
    ) -> Result<Arc<Self>, HttpError> {
        let mut charges = charge(workers, 1).ok_or(HttpError::ConnectionLimitReached)?;
        let exchange = Arc::new(Self {
            state: Mutex::new(State {
                phase: Phase::Sending,
                body: VecDeque::new(),
                body_end: false,
                body_error: false,
                released: false,
            }),
            reader: Condvar::new(),
            wait: Arc::default(),
        });
        let worker = exchange.clone();
        let place = charges.pop();
        thread::spawn(move || {
            let _place = place;
            let timeout = deadline.map(|at| at.saturating_duration_since(Instant::now()));
            match seam.send_request(&request, &connector, timeout) {
                Ok(response) => {
                    worker.state().phase = Phase::Answered(response.head);
                    worker.changed();
                    worker.read_body(response.body);
                }
                Err(err) => {
                    worker.state().phase = Phase::Failed(err);
                    worker.changed();
                }
            }
        });
        Ok(exchange)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn changed(&self) {
        self.reader.notify_all();
        self.wait.wake_up();
    }

    /// Read the body into the buffer while it has room, until it ends, fails,
    /// or the guest lets the exchange go.
    fn read_body(&self, mut body: Box<dyn Read + Send>) {
        let mut chunk = vec![0; 16 * 1024];
        loop {
            let room = {
                let mut state = self.state();
                while state.body.len() >= SOCKET_BUFFER && !state.released {
                    state = self
                        .reader
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if state.released {
                    return;
                }
                SOCKET_BUFFER - state.body.len()
            };
            let want = room.min(chunk.len());
            let read = body.read(&mut chunk[..want]);
            let mut state = self.state();
            let ended = match read {
                Ok(0) => {
                    state.body_end = true;
                    true
                }
                Ok(n) => {
                    state.body.extend(&chunk[..n]);
                    false
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => false,
                Err(_) => {
                    state.body_error = true;
                    true
                }
            };
            drop(state);
            self.changed();
            if ended {
                return;
            }
        }
    }

    /// The response's head, or why it has none; `None` while the request is
    /// under way.
    pub fn response(&self) -> Option<Result<ResponseHead, HttpError>> {
        match &self.state().phase {
            Phase::Sending => None,
            Phase::Answered(head) => Some(Ok(head.clone())),
            Phase::Failed(err) => Some(Err(*err)),
        }
    }

    /// Read what the body holds into `buf`: zero at its end, `WouldBlock`
    /// while it holds nothing yet, and `ConnectionReset` for a body cut short.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        if state.body.is_empty() {
            return match (state.body_end, state.body_error) {
                (_, true) => Err(FilesystemError::ConnectionReset),
                (true, _) => Ok(0),
                _ => Err(FilesystemError::WouldBlock),
            };
        }
        let n = buf.len().min(state.body.len());
        for (slot, byte) in buf.iter_mut().zip(state.body.drain(..n)) {
            *slot = byte;
        }
        drop(state);
        self.changed();
        Ok(n)
    }

    /// What a poll finds: the head arrived, or the body has bytes, its end,
    /// or its failure, registering on `table` for a poll that will sleep.
    pub fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.wait, table);
        let state = self.state();
        let mut mask = PollMask::default();
        if !matches!(state.phase, Phase::Sending)
            && (!state.body.is_empty() || state.body_end || state.body_error)
        {
            mask = mask | PollMask::IN;
        }
        if !matches!(state.phase, Phase::Sending) {
            mask = mask | PollMask::OUT;
        }
        mask
    }

    /// Let the exchange go, so a worker blocked on a full buffer ends.
    pub fn release(&self) {
        self.state().released = true;
        self.changed();
    }
}

/// The default outbound seam: ureq, with no redirect, proxy, or decompression,
/// no status read as an error, and every connection opened by kobako's
/// connector.
#[derive(Default)]
pub struct UreqOutbound;

impl Outbound for UreqOutbound {
    fn send_request(
        &self,
        request: &OutgoingRequest,
        connector: &Connector,
        timeout: Option<Duration>,
    ) -> Result<Response, HttpError> {
        let scheme = match request.scheme {
            Scheme::Http => "http",
            Scheme::Https => return Err(HttpError::ProtocolError),
        };
        let uri = format!(
            "{scheme}://{}{}",
            request.authority, request.path_with_query
        );
        let mut builder = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(uri);
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_slice());
        }
        let http = builder
            .body(request.body.clone())
            .map_err(|_| HttpError::ProtocolError)?;
        let config = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .proxy(None)
            .timeout_global(timeout)
            .build();
        let dial = connector.dial(timeout)?;
        let agent =
            ureq::Agent::with_parts(config, transport::OneShot::new(dial), transport::Unresolved);
        let response = agent.run(http).map_err(to_http_error)?;
        let head = ResponseHead {
            status: response.status().as_u16(),
            headers: response
                .headers()
                .iter()
                .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
                .collect(),
        };
        let body = response.into_body().into_reader();
        Ok(Response {
            head,
            body: Box::new(body),
        })
    }
}

fn to_http_error(err: ureq::Error) -> HttpError {
    match err {
        ureq::Error::Timeout(_) => HttpError::ConnectionTimeout,
        ureq::Error::Io(err) if err.kind() == io::ErrorKind::ConnectionRefused => {
            HttpError::ConnectionRefused
        }
        ureq::Error::Io(_) => HttpError::ConnectionTerminated,
        ureq::Error::Protocol(_) | ureq::Error::BadUri(_) => HttpError::ProtocolError,
        _ => HttpError::Internal,
    }
}

/// The pieces ureq dials through: a connector handing over the one stream
/// kobako's connector opened, and a resolver resolving nothing, so ureq never
/// reaches the network itself.
mod transport {
    use std::fmt;
    use std::sync::Mutex;

    use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
    use ureq::unversioned::transport::{
        Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport,
    };

    use super::{PoisonError, Read, Stream, Write};

    /// A connector handing ureq the stream already opened, once.
    pub(super) struct OneShot(Mutex<Option<Box<dyn Stream>>>);

    impl OneShot {
        pub(super) fn new(stream: Box<dyn Stream>) -> Self {
            Self(Mutex::new(Some(stream)))
        }
    }

    impl fmt::Debug for OneShot {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("OneShot")
        }
    }

    impl Connector for OneShot {
        type Out = StreamTransport;

        fn connect(
            &self,
            details: &ConnectionDetails,
            _chained: Option<()>,
        ) -> Result<Option<Self::Out>, ureq::Error> {
            let stream = self
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .ok_or(ureq::Error::ConnectionFailed)?;
            let buffers = LazyBuffers::new(
                details.config.input_buffer_size(),
                details.config.output_buffer_size(),
            );
            Ok(Some(StreamTransport {
                stream: Mutex::new(stream),
                buffers,
            }))
        }
    }

    /// One stream as ureq's transport.
    /// One stream as ureq's transport, behind a lock since a transport must be
    /// shareable and a stream need not be.
    pub(super) struct StreamTransport {
        stream: Mutex<Box<dyn Stream>>,
        buffers: LazyBuffers,
    }

    impl fmt::Debug for StreamTransport {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("StreamTransport")
        }
    }

    impl Transport for StreamTransport {
        fn buffers(&mut self) -> &mut dyn Buffers {
            &mut self.buffers
        }

        fn transmit_output(
            &mut self,
            amount: usize,
            _timeout: NextTimeout,
        ) -> Result<(), ureq::Error> {
            let output = &self.buffers.output()[..amount];
            let stream = self
                .stream
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner);
            stream.write_all(output)?;
            Ok(())
        }

        fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
            let stream = self
                .stream
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner);
            stream.set_read_timeout(timeout.not_zero().map(|t| *t))?;
            let input = self.buffers.input_append_buf();
            let n = stream.read(input)?;
            self.buffers.input_appended(n);
            Ok(n > 0)
        }

        fn is_open(&mut self) -> bool {
            true
        }
    }

    /// A resolver resolving nothing: the connector dials the authority itself.
    #[derive(Debug)]
    pub(super) struct Unresolved;

    impl Resolver for Unresolved {
        fn resolve(
            &self,
            _uri: &ureq::http::Uri,
            _config: &ureq::config::Config,
            _timeout: NextTimeout,
        ) -> Result<ResolvedSocketAddrs, ureq::Error> {
            let unspecified: std::net::SocketAddr = ([0, 0, 0, 0], 0).into();
            let mut addrs = ResolvedSocketAddrs::from_fn(|_| unspecified);
            addrs.push(unspecified);
            Ok(addrs)
        }
    }
}

impl fmt::Debug for Connector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connector")
            .field("host", &self.host)
            .field("port", &self.port)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    use super::*;
    use crate::net::{HostInet, Namespace, SANDBOX_WORKERS};
    use crate::network::{Builder, Endpoint, Network};
    use crate::security::prepare_creds_within;

    /// A loopback server answering one request with `hello`, handing back the
    /// request line and headers it read.
    fn answering_once() -> (u16, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim_end().is_empty() {
                    break;
                }
                head.push(line.trim_end().to_string());
            }
            (&stream)
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\nx-kind: test\r\n\r\nhello")
                .unwrap();
            head
        });
        (port, server)
    }

    /// A namespace whose one entry holds `endpoints`.
    fn namespace(endpoints: Vec<Endpoint>) -> Namespace {
        let mut entry = Builder::default();
        endpoints.into_iter().for_each(|endpoint| {
            entry.endpoint(endpoint);
        });
        let network = Network::new("web", Arc::new(entry)).unwrap();
        Namespace::new(
            [&network],
            Arc::new(HostInet),
            Arc::new(UreqOutbound),
            Workers::new(SANDBOX_WORKERS),
        )
        .expect("the endpoints compile")
    }

    fn get(authority: &str) -> OutgoingRequest {
        let mut request = OutgoingRequest::new("GET", Scheme::Http, authority);
        request.path_with_query = "/greeting?lang=en".to_string();
        request
    }

    /// The exchange's head, then its whole body.
    fn settled(exchange: &Exchange) -> Result<(ResponseHead, Vec<u8>), HttpError> {
        let head = loop {
            match exchange.response() {
                Some(answer) => break answer?,
                None => thread::sleep(Duration::from_millis(1)),
            }
        };
        let mut body = Vec::new();
        let mut buf = [0; 64];
        loop {
            match exchange.read(&mut buf) {
                Ok(0) => return Ok((head, body)),
                Ok(n) => body.extend_from_slice(&buf[..n]),
                Err(FilesystemError::WouldBlock) => thread::sleep(Duration::from_millis(1)),
                Err(_) => return Err(HttpError::ConnectionTerminated),
            }
        }
    }

    // @behavior NW-037
    #[test]
    fn a_request_to_an_http_endpoint_reads_back_the_response() {
        let (port, server) = answering_once();
        let namespace = namespace(vec![Endpoint::new("localhost", port)
            .unwrap()
            .with_addresses(vec![crate::network::Addresses::Local])]);
        let cred = prepare_creds_within(&[], None);
        let exchange = namespace
            .send(&cred, get(&format!("LocalHost:{port}")), None)
            .expect("the endpoint names the authority");
        let (head, body) = settled(&exchange).expect("the server answers");
        let seen = server.join().unwrap();
        assert!(
            head.status == 200
                && head
                    .headers
                    .contains(&("x-kind".to_string(), b"test".to_vec()))
                && body == b"hello"
                && seen[0] == "GET /greeting?lang=en HTTP/1.1",
            "Namespace::send to an http endpoint must send the request line and read back the \
             status, headers, and body, got {head:?} {body:?} after {seen:?}"
        );
    }

    // @behavior NW-038
    #[test]
    fn a_request_no_http_endpoint_names_is_denied_unsent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let raw = Endpoint::new("127.0.0.1", port)
            .unwrap()
            .with_protocol(Protocol::Tcp);
        let namespace = namespace(vec![raw]);
        let cred = prepare_creds_within(&[], None);
        let answers = [
            format!("127.0.0.1:{port}"),
            format!("elsewhere.example:{port}"),
            format!("198.18.0.0:{port}"),
        ]
        .map(|authority| namespace.send(&cred, get(&authority), None).err());
        thread::sleep(Duration::from_millis(20));
        assert!(
            answers
                .iter()
                .all(|answer| *answer == Some(HttpError::Denied))
                && listener.accept().is_err(),
            "Namespace::send to an authority only a tcp endpoint names, one no endpoint names, \
             or a virtual address must answer Denied, sending nothing, got {answers:?}"
        );
    }

    // @behavior NW-045
    #[test]
    fn a_request_policy_sees_the_request_and_refuses_it_unsent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = seen.clone();
        let mut entry = Builder::default();
        entry
            .endpoint(Endpoint::new("127.0.0.1", port).unwrap())
            .request_policy(move |sent: &crate::network::Request<'_>| {
                recorded.lock().unwrap().push((
                    sent.authority.0.to_string(),
                    sent.method.to_string(),
                    sent.scheme.to_string(),
                    sent.path.to_string(),
                    sent.header_names
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>(),
                ));
                false
            });
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let namespace = Namespace::new(
            [&network],
            Arc::new(HostInet),
            Arc::new(UreqOutbound),
            Workers::new(SANDBOX_WORKERS),
        )
        .unwrap();
        let mut request = get(&format!("127.0.0.1:{port}"));
        request
            .headers
            .push(("x-probe".to_string(), b"secret".to_vec()));
        let cred = prepare_creds_within(&[], None);
        let answer = namespace.send(&cred, request, None).err();
        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            (
                answer,
                seen.lock().unwrap().clone(),
                listener.accept().is_err()
            ),
            (
                Some(HttpError::Denied),
                vec![(
                    "127.0.0.1".to_string(),
                    "GET".to_string(),
                    "http".to_string(),
                    "/greeting".to_string(),
                    vec!["x-probe".to_string()]
                )],
                true
            ),
            "a request policy must see the authority, method, scheme, path with no query, and \
             header names, and its refusal must answer Denied, sending nothing"
        );
    }
}
