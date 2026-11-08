//! An HTTP request the guest sends, as a userspace proxy forwards one: the
//! namespace judges what the guest named on the guest thread, then a task on
//! the reactor hands the request to the outbound seam, whose only dialer is
//! kobako's connector. The response's body fills a buffer the guest reads
//! through the wait queue, so a full buffer stops the task and the TCP window
//! pushes back.

use std::collections::VecDeque;
use std::fmt;
use std::future::{poll_fn, Future};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread;

use bytes::Bytes;
use http_body::Body;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::connections::{open, OpenConnections, Place};
use super::reactor::{Reactor, Ready, Registration, Task};
use super::socket::to_error;
use super::{platform_tls, ProtocolFamily, SOCKET_BUFFER};
use crate::network::{Protocol, Ruleset, Secret};
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
    /// The credentials bound to this request, swapped in only by the default
    /// outbound seam and only on the way out, so no other seam reads a secret.
    pub(crate) swaps: Vec<Swap>,
}

/// One placeholder to replace with a secret in one named header's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Swap {
    pub(crate) header: String,
    pub(crate) placeholder: Vec<u8>,
    pub(crate) secret: Secret,
}

impl Swap {
    /// `value`, the named header's, with each placeholder replaced.
    fn apply(&self, name: &str, value: &[u8]) -> Vec<u8> {
        if !name.eq_ignore_ascii_case(&self.header) || self.placeholder.is_empty() {
            return value.to_vec();
        }
        let mut swapped = Vec::with_capacity(value.len());
        let mut rest = value;
        while let Some(at) = rest
            .windows(self.placeholder.len())
            .position(|window| window == self.placeholder.as_slice())
        {
            swapped.extend_from_slice(&rest[..at]);
            swapped.extend_from_slice(self.secret.expose());
            rest = &rest[at + self.placeholder.len()..];
        }
        swapped.extend_from_slice(rest);
        swapped
    }
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
            swaps: Vec::new(),
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
    /// The response was not one HTTP can read.
    ProtocolError,
    /// The request cannot start now, as a full cap leaves it.
    ConnectionLimitReached,
    /// The server's certificate is not one the host's store trusts.
    TlsCertificate,
    /// TLS could not be agreed with the server.
    TlsProtocol,
    /// The seam failed in a way no other code names.
    Internal,
}

/// A response's head: its status and headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: u16,
    pub headers: Vec<(String, Vec<u8>)>,
}

/// A response's body as the seam streams it.
pub type ResponseBody = UnsyncBoxBody<Bytes, HttpError>;

/// What the outbound seam answers: the head, and the body to read.
pub struct Response {
    pub head: ResponseHead,
    pub body: ResponseBody,
}

/// What the outbound seam's send runs as on the reactor.
pub type Sending = Pin<Box<dyn Future<Output = Result<Response, HttpError>> + Send>>;

/// The replaceable HTTP sender, as wasmtime's `send_request`. It dials only
/// through the connector it is handed; one dialing any other way skips every
/// rule, which is its builder's own choice. The future runs on kobako's
/// reactor, so it must never block.
pub trait Outbound: Send + Sync {
    /// Send `request` through `connector`.
    fn send_request(&self, request: OutgoingRequest, connector: Connector) -> Sending;
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

    /// A connection to the authority, once one of its addresses answers.
    pub async fn dial(&self) -> Result<Transport, HttpError> {
        let addrs = match self.host.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, self.port)],
            Err(_) => {
                let real = self.resolve().await?;
                let local = self.family.local_addresses();
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
            let opened = self
                .family
                .connect(addr)
                .and_then(Transport::new)
                .map_err(|err| to_http_error(&err));
            match opened {
                Ok(connection) => match connection.connected().await {
                    Ok(()) => return Ok(connection),
                    Err(err) => last = to_http_error(&err),
                },
                Err(err) => last = err,
            }
        }
        Err(last)
    }

    /// The host's answer for the authority's name, asked on a thread of its
    /// own, since a resolver blocks.
    async fn resolve(&self) -> Result<Vec<IpAddr>, HttpError> {
        let answer = Arc::new(Mutex::new((None, None::<Waker>)));
        let answering = answer.clone();
        let family = self.family.clone();
        let host = self.host.clone();
        thread::spawn(move || {
            let resolved = family.resolve(&host);
            let mut answer = answering.lock().unwrap_or_else(PoisonError::into_inner);
            answer.0 = Some(resolved);
            if let Some(waker) = answer.1.take() {
                waker.wake();
            }
        });
        let resolved = poll_fn(|cx| {
            let mut answer = answer.lock().unwrap_or_else(PoisonError::into_inner);
            match answer.0.take() {
                Some(resolved) => Poll::Ready(resolved),
                None => {
                    answer.1 = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await;
        match resolved {
            Ok(real) if !real.is_empty() => Ok(real),
            _ => Err(HttpError::DestinationNotFound),
        }
    }
}

/// The TCP connection the connector opened, non-blocking, its readiness
/// watched by the reactor, read and written through tokio's I/O traits.
pub struct Transport {
    stream: mio::net::TcpStream,
    registration: Registration,
    waiters: Arc<Waiters>,
}

/// The task waiting to read and the one waiting to write, woken as the
/// reactor sees the connection change.
#[derive(Default)]
struct Waiters {
    read: Mutex<Option<Waker>>,
    write: Mutex<Option<Waker>>,
}

impl Ready for Waiters {
    fn ready(&self) {
        for waiter in [&self.read, &self.write] {
            if let Some(waker) = waiter.lock().unwrap_or_else(PoisonError::into_inner).take() {
                waker.wake();
            }
        }
    }
}

impl Transport {
    fn new(stream: std::net::TcpStream) -> io::Result<Self> {
        let mut stream = mio::net::TcpStream::from_std(stream);
        let waiters = Arc::new(Waiters::default());
        let ready: Arc<dyn Ready> = waiters.clone();
        let registration = Reactor::current()?.register(&mut stream, Arc::downgrade(&ready))?;
        Ok(Self {
            stream,
            registration,
            waiters,
        })
    }

    /// Once the connect under way settles, as `SO_ERROR` does.
    async fn connected(&self) -> io::Result<()> {
        poll_fn(|cx| {
            self.attempt(cx, false, || {
                if let Some(err) = self.stream.take_error()? {
                    return Err(err);
                }
                match self.stream.peer_addr() {
                    Err(err) if err.kind() == io::ErrorKind::NotConnected => {
                        Err(io::ErrorKind::WouldBlock.into())
                    }
                    other => other.map(drop),
                }
            })
        })
        .await
    }

    /// `op` once it stops answering `WouldBlock`, the waker registered first,
    /// so an event between the two is not lost.
    fn attempt<T>(
        &self,
        cx: &mut Context<'_>,
        read: bool,
        mut op: impl FnMut() -> io::Result<T>,
    ) -> Poll<io::Result<T>> {
        loop {
            match op() {
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                done => return Poll::Ready(done),
            }
            let waiter = if read {
                &self.waiters.read
            } else {
                &self.waiters.write
            };
            *waiter.lock().unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
            match op() {
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Poll::Pending,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                done => return Poll::Ready(done),
            }
        }
    }
}

impl AsyncRead for Transport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let read = this.attempt(cx, true, || {
            io::Read::read(&mut &this.stream, buf.initialize_unfilled())
        });
        read.map(|read| read.map(|n| buf.advance(n)))
    }
}

impl AsyncWrite for Transport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.attempt(cx, false, || io::Write::write(&mut &this.stream, buf))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.stream.shutdown(std::net::Shutdown::Write))
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.registration.deregister(&mut self.stream);
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
    /// The task waiting for room in the body's buffer.
    filler: Option<Waker>,
}

/// One request in flight, then its response: the head once it arrives, and
/// the body as the task reads it.
pub struct Exchange {
    state: Mutex<State>,
    wait: Arc<WaitQueueHead>,
    task: Mutex<Option<Arc<Task>>>,
    /// The request's place against its Sandbox's cap and the process's.
    place: Mutex<Option<Place>>,
}

impl Exchange {
    /// Send `request` through `seam` on the reactor, dialing through
    /// `connector`; it counts against `open_connections` and the process until
    /// released, and a full count answers `ConnectionLimitReached`.
    pub(crate) fn start(
        seam: Arc<dyn Outbound>,
        request: OutgoingRequest,
        connector: Connector,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<Arc<Self>, HttpError> {
        let place = Some(open(open_connections).ok_or(HttpError::ConnectionLimitReached)?);
        let reactor = Reactor::current().map_err(|_| HttpError::Internal)?;
        let exchange = Arc::new(Self {
            state: Mutex::new(State {
                phase: Phase::Sending,
                body: VecDeque::new(),
                body_end: false,
                body_error: false,
                filler: None,
            }),
            wait: Arc::default(),
            task: Mutex::new(None),
            place: Mutex::new(place),
        });
        let filling = Arc::downgrade(&exchange);
        let sending = seam.send_request(request, connector);
        let task = reactor.spawn(async move {
            let answer = sending.await;
            let Some(exchange) = filling.upgrade() else {
                return;
            };
            match answer {
                Ok(response) => {
                    exchange.state().phase = Phase::Answered(response.head);
                    exchange.wait.wake_up();
                    drop(exchange);
                    fill_body(&filling, response.body).await;
                }
                Err(err) => {
                    exchange.state().phase = Phase::Failed(err);
                    exchange.wait.wake_up();
                }
            }
        });
        *exchange.task.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);
        Ok(exchange)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
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
        let filler = state.filler.take();
        drop(state);
        if let Some(filler) = filler {
            filler.wake();
        }
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

    /// Let the exchange go, dropping its task with the connection it holds.
    pub fn release(&self) {
        let task = self
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            task.cancel();
        }
        *self.place.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// Move `body` into the exchange's buffer as room frees, never past the
/// buffer, until the body ends or fails, or the exchange is gone; what a frame
/// holds past the room waits here.
async fn fill_body(exchange: &std::sync::Weak<Exchange>, mut body: ResponseBody) {
    let mut pending = Bytes::new();
    loop {
        if pending.is_empty() {
            let frame = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await;
            let Some(exchange) = exchange.upgrade() else {
                return;
            };
            match frame {
                Some(Ok(frame)) => {
                    pending = frame.into_data().unwrap_or_default();
                    continue;
                }
                Some(Err(_)) => exchange.state().body_error = true,
                None => exchange.state().body_end = true,
            }
            exchange.wait.wake_up();
            return;
        }
        let room = poll_fn(|cx| {
            let Some(exchange) = exchange.upgrade() else {
                return Poll::Ready(None);
            };
            let mut state = exchange.state();
            let room = SOCKET_BUFFER - state.body.len();
            if room > 0 {
                return Poll::Ready(Some(room));
            }
            state.filler = Some(cx.waker().clone());
            Poll::Pending
        })
        .await;
        let (Some(room), Some(exchange)) = (room, exchange.upgrade()) else {
            return;
        };
        let moved = pending.split_to(room.min(pending.len()));
        exchange.state().body.extend(&moved[..]);
        exchange.wait.wake_up();
    }
}

/// The default outbound seam: hyper's HTTP/1 client, with no redirect, proxy,
/// or decompression, every connection opened by kobako's connector and TLS
/// ended on the host. Each seam holds its own TLS configuration, so its
/// sessions end with the Invocation that holds it.
pub struct HyperOutbound {
    tls: Option<Arc<rustls::ClientConfig>>,
}

impl HyperOutbound {
    pub fn new() -> Self {
        Self {
            tls: platform_tls(),
        }
    }
}

impl Default for HyperOutbound {
    fn default() -> Self {
        Self::new()
    }
}

impl Outbound for HyperOutbound {
    fn send_request(&self, request: OutgoingRequest, connector: Connector) -> Sending {
        let tls = self.tls.clone();
        Box::pin(async move {
            let http = to_hyper_request(&request)?;
            let connection = connector.dial().await?;
            match request.scheme {
                Scheme::Http => exchange(Wire(connection), http).await,
                Scheme::Https => {
                    let config = tls.ok_or(HttpError::TlsProtocol)?;
                    let name = rustls::pki_types::ServerName::try_from(
                        connector.authority().0.to_string(),
                    )
                    .map_err(|_| HttpError::ProtocolError)?;
                    let session = tokio_rustls::TlsConnector::from(config)
                        .connect(name, connection)
                        .await
                        .map_err(|err| to_http_error(&err))?;
                    exchange(Wire(session), http).await
                }
            }
        })
    }
}

/// `request` as hyper sends it: the path with its query as the target, each
/// header with its credentials swapped in, and a `host` header naming the
/// authority unless the guest wrote one.
fn to_hyper_request(request: &OutgoingRequest) -> Result<hyper::Request<Full<Bytes>>, HttpError> {
    let mut builder = hyper::Request::builder()
        .method(request.method.as_str())
        .uri(request.path_with_query.as_str());
    if !request
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("host"))
    {
        builder = builder.header("host", request.authority.as_str());
    }
    for (name, value) in &request.headers {
        let value = request
            .swaps
            .iter()
            .fold(value.clone(), |value, swap| swap.apply(name, &value));
        builder = builder.header(name.as_str(), value);
    }
    builder
        .body(Full::new(Bytes::from(request.body.clone())))
        .map_err(|_| HttpError::ProtocolError)
}

/// Send `request` over `wire` as HTTP/1, driving the connection beside it, and
/// answer its response, whose body drives the connection on until it ends.
async fn exchange<T>(
    wire: Wire<T>,
    request: hyper::Request<Full<Bytes>>,
) -> Result<Response, HttpError>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, connection) = hyper::client::conn::http1::handshake(wire)
        .await
        .map_err(|err| to_hyper_error(&err))?;
    let mut connection: Driving = Some(Box::pin(async move {
        let _ = connection.await;
    }));
    let mut sending = Box::pin(sender.send_request(request));
    let response = poll_fn(|cx| {
        drive(&mut connection, cx);
        sending.as_mut().poll(cx)
    })
    .await
    .map_err(|err| to_hyper_error(&err))?;
    let head = ResponseHead {
        status: response.status().as_u16(),
        headers: response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
            .collect(),
    };
    let body = Driven {
        body: response.into_body(),
        connection,
    };
    Ok(Response {
        head,
        body: body.boxed_unsync(),
    })
}

/// The connection's own future, until it ends.
type Driving = Option<Pin<Box<dyn Future<Output = ()> + Send>>>;

fn drive(connection: &mut Driving, cx: &mut Context<'_>) {
    if let Some(running) = connection.as_mut() {
        if running.as_mut().poll(cx).is_ready() {
            *connection = None;
        }
    }
}

/// A response's body, driving its connection as it is read.
struct Driven {
    body: hyper::body::Incoming,
    connection: Driving,
}

impl Body for Driven {
    type Data = Bytes;
    type Error = HttpError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, HttpError>>> {
        let this = self.get_mut();
        drive(&mut this.connection, cx);
        Pin::new(&mut this.body)
            .poll_frame(cx)
            .map_err(|_| HttpError::ConnectionTerminated)
    }
}

/// A connection, plain or under TLS, as hyper reads and writes it.
struct Wire<T>(T);

impl<T: AsyncRead + Unpin> hyper::rt::Read for Wire<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        // SAFETY: `ReadBuf` only writes initialized bytes into the cursor's
        // spare capacity, and `advance` counts no more than it wrote.
        let filled = unsafe {
            let mut read = ReadBuf::uninit(buf.as_mut());
            match Pin::new(&mut self.get_mut().0).poll_read(cx, &mut read) {
                Poll::Ready(Ok(())) => read.filled().len(),
                other => return other,
            }
        };
        unsafe { buf.advance(filled) };
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite + Unpin> hyper::rt::Write for Wire<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

/// The code a failed dial or TLS handshake answers, from the rustls error it
/// carries, if any.
fn to_http_error(err: &io::Error) -> HttpError {
    match err
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        Some(rustls::Error::InvalidCertificate(_)) => return HttpError::TlsCertificate,
        Some(_) => return HttpError::TlsProtocol,
        None => {}
    }
    match err.kind() {
        io::ErrorKind::TimedOut => HttpError::ConnectionTimeout,
        io::ErrorKind::ConnectionRefused => HttpError::ConnectionRefused,
        io::ErrorKind::InvalidData => HttpError::TlsProtocol,
        _ => match to_error(err) {
            FilesystemError::ConnectionReset | FilesystemError::ConnectionAborted => {
                HttpError::ConnectionTerminated
            }
            _ => HttpError::ConnectionRefused,
        },
    }
}

/// The code a failed exchange answers.
fn to_hyper_error(err: &hyper::Error) -> HttpError {
    if err.is_parse() || err.is_parse_status() {
        HttpError::ProtocolError
    } else if err.is_timeout() {
        HttpError::ConnectionTimeout
    } else if err.is_incomplete_message() || err.is_closed() || err.is_canceled() {
        HttpError::ConnectionTerminated
    } else {
        HttpError::Internal
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
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use super::*;
    use crate::net::{HostInet, Namespace, OpenConnections};
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
            Arc::new(HyperOutbound::new()),
            OpenConnections::new(None),
            None,
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
            .send(&cred, get(&format!("LocalHost:{port}")))
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

    // @behavior NW-058
    #[test]
    fn a_body_past_the_buffer_streams_as_it_is_read() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let size = SOCKET_BUFFER * 4;
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut head = [0; 1024];
            let _ = stream.read(&mut head);
            let header = format!("HTTP/1.1 200 OK\r\ncontent-length: {size}\r\n\r\n");
            stream.write_all(header.as_bytes()).unwrap();
            let body = (0..size).map(|i| (i % 251) as u8).collect::<Vec<_>>();
            stream.write_all(&body).unwrap();
        });
        let namespace = namespace(vec![Endpoint::new("127.0.0.1", port).unwrap()]);
        let cred = prepare_creds_within(&[], None);
        let exchange = namespace
            .send(&cred, get(&format!("127.0.0.1:{port}")))
            .expect("an endpoint names the authority");
        let mut held = 0;
        let mut body = Vec::new();
        let mut buf = [0; 4096];
        let started = std::time::Instant::now();
        while body.len() < size && started.elapsed() < Duration::from_secs(10) {
            held = held.max(exchange.state().body.len());
            match exchange.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => body.extend_from_slice(&buf[..n]),
                Err(_) => thread::sleep(Duration::from_millis(1)),
            }
        }
        server.join().unwrap();
        let expected = (0..size).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        assert!(
            body == expected && held <= SOCKET_BUFFER,
            "a response body four buffers long must reach the guest whole and in order, the body \
             waiting for it never past one buffer, got {} bytes, holding up to {held}",
            body.len()
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
        .map(|authority| namespace.send(&cred, get(&authority)).err());
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

    // @behavior NW-048
    #[test]
    fn an_https_request_opens_tls_before_any_byte_of_http() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut first = [0; 1];
            stream.read_exact(&mut first).unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap();
            first[0]
        });
        let namespace = namespace(vec![Endpoint::new("127.0.0.1", port).unwrap()]);
        let cred = prepare_creds_within(&[], None);
        let mut request = get(&format!("127.0.0.1:{port}"));
        request.scheme = Scheme::Https;
        let exchange = namespace
            .send(&cred, request)
            .expect("an endpoint names the authority");
        let answer = settled(&exchange).map(|(head, _)| head.status);
        assert_eq!(
            (server.join().unwrap(), answer),
            (0x16, Err(HttpError::TlsProtocol)),
            "an https request must open TLS with a handshake record before any HTTP, and a server \
             answering in clear must fail it as a TLS protocol error"
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
            Arc::new(HyperOutbound::new()),
            OpenConnections::new(None),
            None,
        )
        .unwrap();
        let mut request = get(&format!("127.0.0.1:{port}"));
        request
            .headers
            .push(("x-probe".to_string(), b"secret".to_vec()));
        let cred = prepare_creds_within(&[], None);
        let answer = namespace.send(&cred, request).err();
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

    /// A seam recording the credentials each request it is handed carries, and
    /// answering none.
    #[derive(Default)]
    struct Recording(Mutex<Vec<Vec<Swap>>>);

    impl Outbound for Recording {
        fn send_request(&self, request: OutgoingRequest, _connector: Connector) -> Sending {
            self.0.lock().unwrap().push(request.swaps);
            Box::pin(async { Err(HttpError::Internal) })
        }
    }

    /// The swaps a request carries once its entry binds a bearer credential to
    /// `exact.example.com` and `*.example.com` on 443, allowing GET on `/api/**`.
    fn swaps_for(scheme: Scheme, host: &str, path: &str, header: &str) -> Vec<Swap> {
        let mut entry = Builder::default();
        entry
            .endpoint(
                Endpoint::new("exact.example.com", 443)
                    .unwrap()
                    .with_rules(vec![crate::network::HttpRule::allow(
                        Some("GET"),
                        "/api/**",
                    )]),
            )
            .endpoint(Endpoint::new("*.example.com", 443).unwrap())
            .credential(crate::network::Credential::new(
                crate::network::Placement::Bearer,
                "kobako-token",
                Arc::new(|_: &crate::network::Request<'_>| {
                    Some(crate::network::Secret::new("s3cret"))
                }),
            ));
        let network = Network::new("api", Arc::new(entry)).unwrap();
        let seam = Arc::new(Recording::default());
        let family = Arc::new(Resolving);
        let namespace = Namespace::new(
            [&network],
            family,
            seam.clone(),
            OpenConnections::new(None),
            None,
        )
        .unwrap();
        let mut request = OutgoingRequest::new("GET", scheme, format!("{host}:443"));
        request.path_with_query = path.to_string();
        request
            .headers
            .push((header.to_string(), b"Bearer kobako-token".to_vec()));
        let exchange = namespace
            .send(&prepare_creds_within(&[], None), request)
            .expect("an endpoint names the authority");
        let _ = settled(&exchange);
        let recorded = seam.0.lock().unwrap().pop().unwrap_or_default();
        recorded
    }

    /// A family resolving every name to loopback and dialing nothing.
    struct Resolving;

    impl ProtocolFamily for Resolving {
        fn connect(&self, _addr: SocketAddr) -> io::Result<std::net::TcpStream> {
            Err(io::ErrorKind::ConnectionRefused.into())
        }
    }

    // @behavior NW-050
    #[test]
    fn a_credential_binds_only_to_https_its_exact_host_and_an_allowed_path() {
        let bound = |scheme, host, path, header| !swaps_for(scheme, host, path, header).is_empty();
        assert_eq!(
            [
                bound(
                    Scheme::Https,
                    "exact.example.com",
                    "/api/repos",
                    "authorization"
                ),
                bound(
                    Scheme::Http,
                    "exact.example.com",
                    "/api/repos",
                    "authorization"
                ),
                bound(
                    Scheme::Https,
                    "other.example.com",
                    "/api/repos",
                    "authorization"
                ),
                bound(
                    Scheme::Https,
                    "exact.example.com",
                    "/admin",
                    "authorization"
                ),
                bound(
                    Scheme::Https,
                    "exact.example.com",
                    "/api/a;b",
                    "authorization"
                ),
                bound(Scheme::Https, "exact.example.com", "/api/repos", "x-other"),
            ],
            [true, false, false, false, false, false],
            "a credential must bind only to https, to its entry's exact host, on a path the \
             endpoint allows and every server reads alike, and only where its header carries the \
             placeholder"
        );
    }

    // @behavior NW-051
    #[test]
    fn a_swap_replaces_the_placeholder_in_its_header_only() {
        let swap = Swap {
            header: "authorization".to_string(),
            placeholder: b"kobako-token".to_vec(),
            secret: crate::network::Secret::new("s3cret"),
        };
        assert_eq!(
            (
                swap.apply("Authorization", b"Bearer kobako-token"),
                swap.apply("x-echo", b"kobako-token"),
                format!("{:?}", swap.secret),
            ),
            (
                b"Bearer s3cret".to_vec(),
                b"kobako-token".to_vec(),
                "Secret(**)".to_string()
            ),
            "a swap must replace the placeholder in its named header only, and a secret must \
             print as a mask"
        );
    }
}
