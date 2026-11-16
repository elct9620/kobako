//! An HTTP request the guest sends, as a userspace proxy forwards one: the
//! namespace judges what the guest named on the guest thread, then a task on
//! the reactor hands the request to the outbound seam, whose only dialer is
//! kobako's connector. The response's body fills a buffer the guest reads
//! through the wait queue, so a full buffer stops the task and the TCP window
//! pushes back.

use std::collections::VecDeque;
use std::future::{poll_fn, Future};
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body::Body;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::connections::{open, OpenConnections, Place};
use super::credential::Swap;
use super::reactor::{Reactor, Task};
use super::socket::to_error;
use super::transport::Connector;
use super::{new_platform_tls, MAX_SOCKET_BUFFER};
use crate::cred::Cred;
use crate::filesystem::anon_inode_getfile;
use crate::memcg::MemoryCharge;
use crate::vfs::{FMode, File, FileOperations, Kiocb, OpenHow, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};
use crate::Errno;

/// Whether a request travels in clear or over TLS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Scheme {
    Http,
    Https,
}

/// The headers no guest sets: wasmtime's `DEFAULT_FORBIDDEN_HEADERS`, and
/// `content-length` and `te`, since the seam frames the body and its encoding
/// itself.
const FORBIDDEN_HEADERS: [&str; 11] = [
    "connection",
    "content-length",
    "host",
    "http2-settings",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "transfer-encoding",
    "upgrade",
];

/// Why a header the guest names is not one it may set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldError {
    InvalidSyntax,
    Forbidden,
    /// The list would outgrow `MAX_FIELD_SIZE`.
    TooLarge,
    /// The list would outgrow the Invocation's memory.
    OutOfMemory,
}

/// The most room one header list the guest builds may take, as wasmtime's
/// `DEFAULT_FIELD_SIZE_LIMIT`; each entry counts its name, its value, and its
/// own place, so even an empty value counts.
pub const MAX_FIELD_SIZE: usize = 128 * 1024;

/// The most a response's head, or one read of its body, takes in the seam's
/// read buffer, as hyper's `max_buf_size` bounds it, so a head the guest could
/// read as a header list fits and a longer one answers a protocol error. An
/// exchange holds it against the Invocation's memory.
const MAX_READ_BUFFER: usize = MAX_FIELD_SIZE;

/// A header list the guest builds or reads, held on its behalf against the
/// Invocation's memory, as a request's body is, so no list grows the host
/// past what the guest was allotted.
pub struct Fields {
    list: Mutex<List>,
}

/// A list's entries and the room they take.
struct List {
    entries: Vec<(String, Vec<u8>)>,
    room: MemoryCharge,
}

/// The room `name` and `value` take in a list.
fn room_of(name: &str, value: &[u8]) -> usize {
    name.len() + value.len() + std::mem::size_of::<(String, Vec<u8>)>()
}

/// The room every entry of `entries` takes in a list.
fn total_room_of(entries: &[(String, Vec<u8>)]) -> usize {
    entries
        .iter()
        .map(|(name, value)| room_of(name, value))
        .sum()
}

impl Fields {
    /// An empty list, charged to `cred` as it grows.
    pub fn new(cred: Arc<Cred>) -> Arc<Self> {
        Arc::new(Self {
            list: Mutex::new(List {
                entries: Vec::new(),
                room: MemoryCharge::empty(cred.memory_budget()),
            }),
        })
    }

    /// A list holding `entries`, as a response's headers are handed out, its
    /// room charged to `cred`; it may outgrow `MAX_FIELD_SIZE`, which only
    /// bounds what the guest appends.
    pub fn with_entries(
        cred: Arc<Cred>,
        entries: Vec<(String, Vec<u8>)>,
    ) -> Result<Arc<Self>, FieldError> {
        let room = total_room_of(&entries);
        let room =
            MemoryCharge::new(cred.memory_budget(), room).map_err(|_| FieldError::OutOfMemory)?;
        Ok(Arc::new(Self {
            list: Mutex::new(List { entries, room }),
        }))
    }

    fn list(&self) -> MutexGuard<'_, List> {
        self.list.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Append the header `name` with `value`, once `to_field_name` and
    /// `to_field_value` take them and the list keeps within `MAX_FIELD_SIZE`
    /// and the Invocation's memory.
    pub fn append(&self, name: &str, value: Vec<u8>) -> Result<(), FieldError> {
        let name = to_field_name(name)?;
        let value = to_field_value(value)?;
        let room = room_of(&name, &value);
        let mut list = self.list();
        let grown = list.room.bytes() + room;
        if grown > MAX_FIELD_SIZE {
            return Err(FieldError::TooLarge);
        }
        list.room
            .resize(grown)
            .map_err(|_| FieldError::OutOfMemory)?;
        list.entries.push((name, value));
        Ok(())
    }

    /// Every entry, in the order appended.
    pub fn entries(&self) -> Vec<(String, Vec<u8>)> {
        self.list().entries.clone()
    }
}

/// Whether `text` is an HTTP token, as a method or a header name must be.
pub fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

/// Whether `text` is a URI scheme, as hyper's `Scheme` parses one.
pub fn is_scheme(text: &str) -> bool {
    text.parse::<hyper::http::uri::Scheme>().is_ok()
}

/// The header name a guest may set, lowercase: a token, and none the seam
/// writes itself.
pub fn to_field_name(name: &str) -> Result<String, FieldError> {
    if !is_token(name) {
        return Err(FieldError::InvalidSyntax);
    }
    let name = name.to_ascii_lowercase();
    match FORBIDDEN_HEADERS.contains(&name.as_str()) {
        true => Err(FieldError::Forbidden),
        false => Ok(name),
    }
}

/// The header value a guest may set: one an HTTP field value holds, with no
/// control byte but a tab, as hyper's `HeaderValue` and wasmtime take one.
pub fn to_field_value(value: Vec<u8>) -> Result<Vec<u8>, FieldError> {
    match hyper::header::HeaderValue::from_bytes(&value) {
        Ok(_) => Ok(value),
        Err(_) => Err(FieldError::InvalidSyntax),
    }
}

/// Whether `authority` names a host alone, with no path or user part, and a
/// port, when it names one, as a number that fits, as wasmtime's
/// `parse_authority` asks.
pub fn is_authority(authority: &str) -> bool {
    if authority.contains('@') {
        return false;
    }
    let has_port = match authority.rfind(']') {
        Some(at) => authority[at..].contains(':'),
        None => authority.contains(':'),
    };
    hyper::http::uri::Authority::try_from(authority)
        .is_ok_and(|parsed| !has_port || parsed.port_u16().is_some())
}

/// The host and port an authority names, the port defaulting by `scheme`;
/// `None` for one `is_authority` refuses. One parser reads every authority,
/// so the authority a request is judged by is the one it reaches.
pub(crate) fn split_authority(authority: &str, scheme: Scheme) -> Option<(String, u16)> {
    if !is_authority(authority) {
        return None;
    }
    let parsed = hyper::http::uri::Authority::try_from(authority).ok()?;
    let host = parsed.host().trim_start_matches('[').trim_end_matches(']');
    (!host.is_empty()).then(|| {
        (
            host.to_ascii_lowercase(),
            parsed.port_u16().unwrap_or(default_port(scheme)),
        )
    })
}

/// The authority a request names its server by, from the `host` and `port`
/// it was judged for: the port only when it is not `scheme`'s default, as
/// RFC 9110's `Host` field writes one.
pub(crate) fn join_authority(host: &str, port: u16, scheme: Scheme) -> String {
    match port == default_port(scheme) {
        true => host.to_string(),
        false => format!("{host}:{port}"),
    }
}

/// The port `scheme` reaches when an authority names none.
fn default_port(scheme: Scheme) -> u16 {
    match scheme {
        Scheme::Http => 80,
        Scheme::Https => 443,
    }
}

/// `path` as the request target sent in origin form, as wasmtime takes one:
/// `""` is `/` and a query alone sits on `/`; anything else not starting with
/// `/`, or no URI path holds, is none. A fragment is cut, as the target sent
/// carries none, so every rule judges what the server reads.
pub fn to_origin_form(path: &str) -> Option<String> {
    let target = match path.chars().next() {
        None => "/".to_string(),
        Some('?') => format!("/{path}"),
        Some('/') => path.to_string(),
        Some(_) => return None,
    };
    hyper::http::uri::PathAndQuery::try_from(target)
        .ok()
        .map(|target| target.as_str().to_string())
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
    pub timeouts: Timeouts,
    /// The credentials bound to this request, swapped in only by the default
    /// outbound seam and only on the way out, so no other seam reads a secret.
    pub(crate) swaps: Vec<Swap>,
}

/// How long a request waits at each step, as `wasi:http`'s `request-options`
/// names them; `None` waits `DEFAULT_TIMEOUT`. Each bounds its own wait, as
/// wasmtime-wasi-http's does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Timeouts {
    /// For the connect to the server, answered as `ConnectionTimeout`.
    pub connect: Option<Duration>,
    /// For the response's head once connected, answered as
    /// `ConnectionReadTimeout`.
    pub first_byte: Option<Duration>,
    /// For each part of the body after the last, failing the body's read as
    /// `ETIMEDOUT`.
    pub between_bytes: Option<Duration>,
}

/// How long a wait the request sets no timeout for lasts, as
/// wasmtime-wasi-http's `default_send_request` bounds each, so a server that
/// never answers ends the wait even where the run itself has no deadline.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// The bound a wait takes from its timeout, `DEFAULT_TIMEOUT` where none is
/// set.
fn bound(timeout: Option<Duration>) -> Duration {
    timeout.unwrap_or(DEFAULT_TIMEOUT)
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
            timeouts: Timeouts::default(),
            swaps: Vec::new(),
        }
    }

    /// The bytes its method, authority, path, and headers hold, as a header
    /// list counts its entries.
    fn room(&self) -> usize {
        self.method.len()
            + self.authority.len()
            + self.path_with_query.len()
            + total_room_of(&self.headers)
    }
}

/// The body a guest writes for a request, its bytes held on the guest's behalf
/// against the Invocation's memory, as `vm_committed_as` counts a mapping, so a
/// guest cannot grow the host past what it was allotted.
pub struct RequestBody {
    cred: Arc<Cred>,
    written: Mutex<WrittenBody>,
}

/// What a body holds and the room it takes.
struct WrittenBody {
    bytes: Vec<u8>,
    room: MemoryCharge,
}

impl RequestBody {
    /// An empty body, its bytes charged to `cred`.
    pub fn new(cred: Arc<Cred>) -> Arc<Self> {
        let room = MemoryCharge::empty(cred.memory_budget());
        Arc::new(Self {
            cred,
            written: Mutex::new(WrittenBody {
                bytes: Vec::new(),
                room,
            }),
        })
    }

    fn written(&self) -> MutexGuard<'_, WrittenBody> {
        self.written.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Allocate the file the guest writes the body through, as
    /// `anon_inode_getfile` does; it is always ready for a write.
    pub fn alloc_file(self: &Arc<Self>) -> File {
        let how = OpenHow {
            write: true,
            nonblock: true,
            ..OpenHow::default()
        };
        anon_inode_getfile(Box::new(RequestBodyFile(self.clone())), &how, &self.cred)
    }

    /// Append `buf`, refused past the Invocation's memory as `ENOMEM`.
    fn write(&self, buf: &[u8]) -> Result<usize, Errno> {
        let mut written = self.written();
        let grown = written.bytes.len() + buf.len();
        written.room.resize(grown)?;
        written.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    /// Take what was written, given back to the Invocation's memory, as the
    /// exchange that sends it reserves it again.
    pub fn take(&self) -> Vec<u8> {
        let mut written = self.written();
        let bytes = std::mem::take(&mut written.bytes);
        written.room = MemoryCharge::empty(self.cred.memory_budget());
        bytes
    }
}

/// What answers for a request body's file.
struct RequestBodyFile(Arc<RequestBody>);

impl FileOperations for RequestBodyFile {
    fn write_iter(&mut self, _iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, Errno> {
        self.0.write(buf)
    }

    fn poll(&self, _mode: FMode, _table: Option<&mut PollTable>) -> PollMask {
        PollMask::OUT
    }

    fn is_stream(&self) -> bool {
        true
    }
}

/// Why a request has no response, as WASI's `wasi:http` error codes name each
/// a client may meet; none carries host text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HttpError {
    /// No entry lets the guest send it.
    Denied,
    /// The authority's name has no address.
    DnsError,
    /// No route reaches the authority's address.
    DestinationUnavailable,
    /// The authority refused the connection.
    ConnectionRefused,
    /// The connection did not open in time.
    ConnectionTimeout,
    /// The response's head did not arrive in time.
    ConnectionReadTimeout,
    /// The connection ended before the response did.
    ConnectionTerminated,
    /// The response was not one HTTP can read.
    ProtocolError,
    /// The request cannot start now, as a full cap leaves it.
    ConnectionLimitReached,
    /// The server's certificate is not one the host's store trusts.
    TlsCertificate,
    /// The server ended TLS with the alert of this id.
    TlsAlertReceived(u8),
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
pub type ResponseFuture = Pin<Box<dyn Future<Output = Result<Response, HttpError>> + Send>>;

/// The replaceable HTTP sender, as wasmtime's `send_request`. It dials only
/// through the connector it is handed; one dialing any other way skips every
/// rule, which is its builder's own choice. The future runs on kobako's
/// reactor, so it must never block.
pub trait Outbound: Send + Sync {
    /// Send `request` through `connector`.
    fn send_request(&self, request: OutgoingRequest, connector: Connector) -> ResponseFuture;
}

/// What a timeout ends: the request, failed as the error names, or the body's
/// next read.
#[derive(Clone, Copy)]
enum Expiry {
    Fail(HttpError),
    Body,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Sending,
    Answered(ResponseHead),
    Failed(HttpError),
}

struct State {
    phase: Phase,
    /// What the response's head holds, reserved against the Invocation's
    /// memory as it arrives and given back with the head, as Linux uncharges
    /// an object's memory only when the object is freed.
    head_memory: Option<MemoryCharge>,
    body: VecDeque<u8>,
    body_ended: bool,
    body_failed: bool,
    /// The body waited past its timeout for its next part.
    body_timed_out: bool,
    /// When the head or the body's last part arrived.
    last_part: Option<Instant>,
    /// The task waiting for room in the body's buffer.
    filler: Option<Waker>,
}

impl State {
    /// Whether the body holds nothing yet and may still bring a part: it
    /// neither ended, failed, nor waited past its timeout.
    fn is_awaiting_body(&self) -> bool {
        self.body.is_empty() && !self.body_ended && !self.body_failed && !self.body_timed_out
    }
}

/// One request in flight, then its response: the head once it arrives, and
/// the body as the task reads it.
pub struct Exchange {
    state: Arc<Mutex<State>>,
    wait: Arc<WaitQueueHead>,
    task: Mutex<Option<Arc<Task>>>,
    /// The request's place against its Sandbox's cap and the process's.
    place: Mutex<Option<Arc<Place>>>,
    /// What the request, the seam's read buffer, and the response's buffer
    /// hold, reserved against the Invocation's memory until the exchange is
    /// released.
    memory: Mutex<Option<MemoryCharge>>,
    timeouts: Timeouts,
    started: Instant,
    /// When the connector's connect opened, which starts the wait for the
    /// head.
    connected: Arc<OnceLock<Instant>>,
}

impl Exchange {
    /// Send `request` through `seam` on the reactor, dialing through the
    /// connector `connector_for` makes with the request's place; it counts
    /// against `open_connections` and the process until
    /// released and its connection closed, and a full count answers
    /// `ConnectionLimitReached`. The
    /// request, the seam's read buffer, and the response's buffer count
    /// against `cred`'s memory until then.
    pub(crate) fn start(
        seam: Arc<dyn Outbound>,
        request: OutgoingRequest,
        connector_for: impl FnOnce(Arc<Place>) -> Connector,
        open_connections: &Arc<OpenConnections>,
        cred: &Arc<Cred>,
    ) -> Result<Arc<Self>, HttpError> {
        let room = request.room() + request.body.len() + MAX_READ_BUFFER + MAX_SOCKET_BUFFER;
        let memory =
            MemoryCharge::new(cred.memory_budget(), room).map_err(|_| HttpError::Internal)?;
        let place = Arc::new(open(open_connections).ok_or(HttpError::ConnectionLimitReached)?);
        let mut connector = connector_for(place.clone());
        let place = Some(place);
        let reactor = Reactor::current().map_err(|_| HttpError::Internal)?;
        let exchange = Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                phase: Phase::Sending,
                head_memory: None,
                body: VecDeque::new(),
                body_ended: false,
                body_failed: false,
                body_timed_out: false,
                last_part: None,
                filler: None,
            })),
            wait: Arc::default(),
            task: Mutex::new(None),
            place: Mutex::new(place),
            memory: Mutex::new(Some(memory)),
            timeouts: request.timeouts,
            started: Instant::now(),
            connected: connector.connected.clone(),
        });
        // The task holds what it answers into, never the exchange, so the
        // exchange's last hold, which cancels the task, is never the task's.
        let (state, wait) = (exchange.state.clone(), exchange.wait.clone());
        let cred = cred.clone();
        connector.wait = Some(wait.clone());
        let sending = seam.send_request(request, connector);
        let task = reactor.spawn(async move {
            match sending.await {
                Ok(response) => {
                    let room = total_room_of(&response.head.headers);
                    let Ok(head_memory) = MemoryCharge::new(cred.memory_budget(), room) else {
                        lock(&state).phase = Phase::Failed(HttpError::Internal);
                        wait.wake_up();
                        return;
                    };
                    {
                        let mut answered = lock(&state);
                        // A head arriving past its timeout finds the exchange
                        // failed already.
                        if answered.phase != Phase::Sending {
                            return;
                        }
                        answered.head_memory = Some(head_memory);
                        answered.phase = Phase::Answered(response.head);
                        answered.last_part = Some(Instant::now());
                    }
                    wait.wake_up();
                    fill_body(&state, &wait, response.body).await;
                }
                Err(err) => {
                    lock(&state).phase = Phase::Failed(err);
                    wait.wake_up();
                }
            }
        });
        *exchange.task.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);
        Ok(exchange)
    }

    /// The exchange's state, once any timeout that passed has ended its wait,
    /// as a timeout read on its next look rather than by a timer.
    fn state(&self) -> MutexGuard<'_, State> {
        let mut state = lock(&self.state);
        if self.expire(&mut state) {
            drop(state);
            self.cancel_task();
            state = lock(&self.state);
        }
        state
    }

    /// Drop the task sending the request, with the connection it holds.
    fn cancel_task(&self) {
        let task = self
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            task.cancel();
        }
    }

    /// The soonest a timeout ends the wait under way: the connect until it
    /// opens, then the head, then each part of the body; `None` while nothing
    /// is awaited.
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline_of(&lock(&self.state)).map(|(at, _)| at)
    }

    fn deadline_of(&self, state: &State) -> Option<(Instant, Expiry)> {
        match state.phase {
            Phase::Sending => match self.connected.get() {
                None => Some((
                    self.started + bound(self.timeouts.connect),
                    Expiry::Fail(HttpError::ConnectionTimeout),
                )),
                Some(at) => Some((
                    *at + bound(self.timeouts.first_byte),
                    Expiry::Fail(HttpError::ConnectionReadTimeout),
                )),
            },
            Phase::Answered(_) if state.is_awaiting_body() => {
                let after = bound(self.timeouts.between_bytes);
                state.last_part.map(|at| (at + after, Expiry::Body))
            }
            Phase::Answered(_) | Phase::Failed(_) => None,
        }
    }

    /// End the wait under way once its timeout passed, answering whether it
    /// did, so the task sending the request can be let go.
    fn expire(&self, state: &mut State) -> bool {
        let Some((at, expiry)) = self.deadline_of(state) else {
            return false;
        };
        if Instant::now() < at {
            return false;
        }
        match expiry {
            Expiry::Fail(err) => state.phase = Phase::Failed(err),
            Expiry::Body => state.body_timed_out = true,
        }
        true
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

    /// Allocate the file a poll finds readable once the response's head
    /// arrives or the request fails, as `anon_inode_getfile` does.
    pub fn alloc_response_file(self: &Arc<Self>, cred: &Arc<Cred>) -> File {
        let how = OpenHow {
            read: true,
            nonblock: true,
            ..OpenHow::default()
        };
        anon_inode_getfile(Box::new(ResponseFile(self.clone())), &how, cred)
    }

    /// Allocate the file the response's body reads from, as
    /// `anon_inode_getfile` does.
    pub fn alloc_body_file(self: &Arc<Self>, cred: &Arc<Cred>) -> File {
        let how = OpenHow {
            read: true,
            nonblock: true,
            ..OpenHow::default()
        };
        anon_inode_getfile(Box::new(BodyFile(self.clone())), &how, cred)
    }

    /// Read what the body holds into `buf`: zero at its end, `WouldBlock`
    /// while it holds nothing yet, `ConnectionReset` for a body cut short, and
    /// `TimedOut` once its next part waited past its timeout.
    fn read(&self, buf: &mut [u8]) -> Result<usize, Errno> {
        let mut state = self.state();
        if state.body.is_empty() {
            return match (state.body_ended, state.body_failed, state.body_timed_out) {
                (_, true, _) => Err(Errno::ConnectionReset),
                (_, _, true) => Err(Errno::TimedOut),
                (true, _, _) => Ok(0),
                _ => Err(Errno::WouldBlock),
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

    /// Let the exchange go, dropping its task with the connection it holds
    /// and the body's buffer. The head stays readable, and held against the
    /// Invocation's memory, while the guest keeps the exchange.
    pub fn release(&self) {
        self.cancel_task();
        lock(&self.state).body = VecDeque::new();
        *self.place.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *self.memory.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// The last hold on an exchange lets it go, as the last `fput` of a socket's
/// file releases it, so no exchange outlives what the guest holds of it.
impl Drop for Exchange {
    fn drop(&mut self) {
        self.release();
    }
}

/// What answers for the file an exchange's response is awaited on.
struct ResponseFile(Arc<Exchange>);

impl FileOperations for ResponseFile {
    fn poll(&self, _mode: FMode, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.0.wait, table);
        match self.0.state().phase {
            Phase::Sending => PollMask::default(),
            Phase::Answered(_) | Phase::Failed(_) => PollMask::IN,
        }
    }

    fn is_stream(&self) -> bool {
        true
    }
}

/// What answers for the file an exchange's body is read from.
struct BodyFile(Arc<Exchange>);

impl FileOperations for BodyFile {
    fn read_iter(&mut self, _iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, Errno> {
        self.0.read(buf)
    }

    fn poll(&self, _mode: FMode, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.0.wait, table);
        let state = self.0.state();
        let settled = !matches!(state.phase, Phase::Sending);
        match settled && !state.is_awaiting_body() {
            true => PollMask::IN,
            false => PollMask::default(),
        }
    }

    fn is_stream(&self) -> bool {
        true
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Move `body` into the exchange's buffer as room frees, never past the
/// buffer, until the body ends or fails, or the exchange lets the task go;
/// what a frame holds past the room waits here.
async fn fill_body(state: &Mutex<State>, wait: &WaitQueueHead, mut body: ResponseBody) {
    let mut pending = Bytes::new();
    loop {
        if pending.is_empty() {
            match poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
                Some(Ok(frame)) => {
                    pending = frame.into_data().unwrap_or_default();
                    continue;
                }
                Some(Err(_)) => lock(state).body_failed = true,
                None => lock(state).body_ended = true,
            }
            wait.wake_up();
            return;
        }
        let room = poll_fn(|cx| {
            let mut state = lock(state);
            let room = MAX_SOCKET_BUFFER - state.body.len();
            if room > 0 {
                return Poll::Ready(room);
            }
            state.filler = Some(cx.waker().clone());
            Poll::Pending
        })
        .await;
        let moved = pending.split_to(room.min(pending.len()));
        let mut filled = lock(state);
        filled.body.extend(&moved[..]);
        filled.last_part = Some(Instant::now());
        drop(filled);
        wait.wake_up();
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
            tls: new_platform_tls(),
        }
    }
}

impl Default for HyperOutbound {
    fn default() -> Self {
        Self::new()
    }
}

impl Outbound for HyperOutbound {
    fn send_request(&self, request: OutgoingRequest, connector: Connector) -> ResponseFuture {
        let tls = match request.scheme {
            Scheme::Http => None,
            Scheme::Https => match self.tls.clone() {
                Some(config) => Some(config),
                None => return Box::pin(async { Err(HttpError::TlsProtocol) }),
            },
        };
        Box::pin(async move {
            let http = to_hyper_request(request)?;
            let connection = connector.dial().await?;
            match tls {
                None => send_over(Wire(connection), http).await,
                Some(config) => {
                    let name = rustls::pki_types::ServerName::try_from(connector.authority().0)
                        .map_err(|_| HttpError::ProtocolError)?;
                    let session = tokio_rustls::TlsConnector::from(config)
                        .connect(name, connection)
                        .await
                        .map_err(|err| HttpError::from_tls(&err))?;
                    send_over(Wire(session), http).await
                }
            }
        })
    }
}

/// `request` as hyper sends it: the path with its query as the target, a
/// `host` header naming the authority, and each header with its credentials
/// swapped in.
fn to_hyper_request(request: OutgoingRequest) -> Result<hyper::Request<Full<Bytes>>, HttpError> {
    let mut builder = hyper::Request::builder()
        .method(request.method.as_str())
        .uri(request.path_with_query.as_str())
        .header("host", request.authority.as_str());
    for (name, value) in &request.headers {
        let value = request
            .swaps
            .iter()
            .fold(value.clone(), |value, swap| swap.apply(name, &value));
        builder = builder.header(name.as_str(), value);
    }
    builder
        .body(Full::new(Bytes::from(request.body)))
        .map_err(|_| HttpError::ProtocolError)
}

/// Send `request` over `wire` as HTTP/1, driving the connection beside it, and
/// answer its response, whose body drives the connection on until it ends.
async fn send_over<T>(
    wire: Wire<T>,
    request: hyper::Request<Full<Bytes>>,
) -> Result<Response, HttpError>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
        .max_buf_size(MAX_READ_BUFFER)
        .handshake(wire)
        .await
        .map_err(|err| HttpError::from_hyper(&err))?;
    let mut connection: ConnectionTask = Some(Box::pin(async move {
        let _ = connection.await;
    }));
    let mut sending = Box::pin(sender.send_request(request));
    let response = poll_fn(|cx| {
        drive(&mut connection, cx);
        sending.as_mut().poll(cx)
    })
    .await
    .map_err(|err| HttpError::from_hyper(&err))?;
    let head = ResponseHead {
        status: response.status().as_u16(),
        headers: response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
            .collect(),
    };
    let body = DrivenBody {
        body: response.into_body(),
        connection,
    };
    Ok(Response {
        head,
        body: body.boxed_unsync(),
    })
}

/// The connection's own future, until it ends.
type ConnectionTask = Option<Pin<Box<dyn Future<Output = ()> + Send>>>;

fn drive(connection: &mut ConnectionTask, cx: &mut Context<'_>) {
    if let Some(running) = connection.as_mut() {
        if running.as_mut().poll(cx).is_ready() {
            *connection = None;
        }
    }
}

/// A response's body, driving its connection as it is read.
struct DrivenBody {
    body: hyper::body::Incoming,
    connection: ConnectionTask,
}

impl Body for DrivenBody {
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

impl HttpError {
    /// The code a failed TLS handshake answers, as wasmtime-wasi-http's
    /// `p2_error_from_tls` names each: the rustls error it carries, a
    /// connection the server ended as `connection-terminated`, and any other
    /// as `TLS-protocol-error`; memory the socket could not reserve is the
    /// seam's own failure.
    pub(crate) fn from_tls(err: &io::Error) -> Self {
        match err
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        {
            Some(rustls::Error::InvalidCertificate(_)) => return Self::TlsCertificate,
            Some(rustls::Error::AlertReceived(alert)) => {
                return Self::TlsAlertReceived(u8::from(*alert))
            }
            Some(_) => return Self::TlsProtocol,
            None => {}
        }
        if err.kind() == io::ErrorKind::UnexpectedEof {
            return Self::ConnectionTerminated;
        }
        match to_error(err) {
            Errno::ConnectionReset | Errno::ConnectionAborted | Errno::BrokenPipe => {
                Self::ConnectionTerminated
            }
            Errno::OutOfMemory => Self::Internal,
            _ => Self::TlsProtocol,
        }
    }

    /// The code a connection failing with `err` answers, as wasmtime-wasi-http
    /// names each; memory the socket could not reserve is the seam's own
    /// failure.
    pub(crate) fn from_errno(err: &Errno) -> Self {
        match err {
            Errno::OutOfMemory => Self::Internal,
            Errno::TimedOut => Self::ConnectionTimeout,
            Errno::HostUnreachable
            | Errno::NetworkUnreachable
            | Errno::NetworkDown
            | Errno::AddressFamilyNotSupported => Self::DestinationUnavailable,
            Errno::ConnectionReset | Errno::ConnectionAborted => Self::ConnectionTerminated,
            _ => Self::ConnectionRefused,
        }
    }

    /// The code a failed exchange answers.
    fn from_hyper(err: &hyper::Error) -> Self {
        if err.is_parse() || err.is_parse_status() {
            Self::ProtocolError
        } else if err.is_timeout() {
            Self::ConnectionTimeout
        } else if err.is_incomplete_message() || err.is_closed() || err.is_canceled() {
            Self::ConnectionTerminated
        } else {
            Self::Internal
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use std::net::SocketAddr;

    use super::super::family::testing::{CountsDials, StallsLookups};
    use super::*;
    use crate::net::{HostInet, Namespace, OpenConnections, ProtocolFamily};
    use crate::network::Protocol;
    use crate::network::{Builder, Endpoint, Network};
    use crate::security::prepare_creds;

    #[test]
    fn a_failed_dial_answers_the_code_its_host_error_names() {
        let dials = [
            io::ErrorKind::TimedOut,
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::HostUnreachable,
            io::ErrorKind::NetworkUnreachable,
            io::ErrorKind::NetworkDown,
        ]
        .map(|kind| HttpError::from_errno(&to_error(&io::Error::from(kind))));
        assert_eq!(
            dials,
            [
                HttpError::ConnectionTimeout,
                HttpError::ConnectionRefused,
                HttpError::ConnectionTerminated,
                HttpError::ConnectionTerminated,
                HttpError::DestinationUnavailable,
                HttpError::DestinationUnavailable,
                HttpError::DestinationUnavailable,
            ],
            "a dial that timed out, was refused, reset, aborted, or found no route to its host or \
             network must answer connection-timeout, connection-refused, connection-terminated \
             twice, and destination-unavailable three times, as wasmtime-wasi-http names each"
        );
    }

    // @behavior NW-225
    #[test]
    fn a_failed_tls_handshake_answers_the_code_wasmtime_wasi_http_gives_it() {
        let alert = io::Error::new(
            io::ErrorKind::InvalidData,
            rustls::Error::AlertReceived(rustls::AlertDescription::HandshakeFailure),
        );
        let handshakes = [
            io::ErrorKind::UnexpectedEof,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::InvalidData,
            io::ErrorKind::TimedOut,
        ]
        .map(|kind| HttpError::from_tls(&io::Error::from(kind)));
        assert_eq!(
            (handshakes, HttpError::from_tls(&alert)),
            (
                [
                    HttpError::ConnectionTerminated,
                    HttpError::ConnectionTerminated,
                    HttpError::ConnectionTerminated,
                    HttpError::TlsProtocol,
                    HttpError::TlsProtocol,
                ],
                HttpError::TlsAlertReceived(40)
            ),
            "a TLS handshake the server ended by EOF, reset, or a broken pipe must answer \
             connection-terminated, bytes TLS could not read or any other failure \
             TLS-protocol-error, and a handshake_failure alert TLS-alert-received with its id, as \
             wasmtime-wasi-http's p2_error_from_tls names each"
        );
    }

    /// A loopback server answering one request with `hello`, handing back the
    /// request line and headers it read.
    fn serve_once() -> (u16, thread::JoinHandle<Vec<String>>) {
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
        Namespace::for_network(&network, Arc::new(HostInet), Arc::new(HyperOutbound::new()))
    }

    fn greeting_request(authority: &str) -> OutgoingRequest {
        let mut request = OutgoingRequest::new("GET", Scheme::Http, authority);
        request.path_with_query = "/greeting?lang=en".to_string();
        request
    }

    /// The exchange's head, then its whole body.
    fn read_response(exchange: &Exchange) -> Result<(ResponseHead, Vec<u8>), HttpError> {
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
                Err(Errno::WouldBlock) => thread::sleep(Duration::from_millis(1)),
                Err(_) => return Err(HttpError::ConnectionTerminated),
            }
        }
    }

    // @behavior NW-225
    #[test]
    fn a_request_to_a_name_with_no_address_answers_dns_error_undialed() {
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("missing.test", 80).unwrap());
        let network = Network::new("web", Arc::new(entry)).unwrap();
        // A Mock answers a name it holds no route for with no address.
        let mock = Arc::new(crate::net::Mock::default());
        let namespace = Namespace::for_network(
            &network,
            mock.clone(),
            Arc::new(HyperOutbound { tls: None }),
        );
        let exchange = crate::net::send(
            &namespace,
            &prepare_creds(&[], None),
            greeting_request("missing.test"),
        )
        .expect("the endpoint admits it");
        assert_eq!(
            (read_response(&exchange).err(), mock.dialed_addresses()),
            (Some(HttpError::DnsError), Vec::new()),
            "a request to a name its resolver finds no address for must answer DNS-error, as \
             wasmtime-wasi-http answers a failed lookup, and dial nothing"
        );
    }

    #[test]
    fn an_https_request_without_trust_roots_dials_nothing() {
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("127.0.0.1", 443).unwrap());
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let family = Arc::new(CountsDials::default());
        let namespace = Namespace::for_network(
            &network,
            family.clone(),
            Arc::new(HyperOutbound { tls: None }),
        );
        let cred = prepare_creds(&[], None);
        let mut request = OutgoingRequest::new("GET", Scheme::Https, "127.0.0.1:443");
        request.path_with_query = "/".to_string();
        let exchange =
            crate::net::send(&namespace, &cred, request).expect("the endpoint admits it");
        let answer = loop {
            match exchange.response() {
                Some(answer) => break answer,
                None => thread::sleep(Duration::from_millis(1)),
            }
        };
        assert_eq!(
            (answer.err(), family.dials()),
            (Some(HttpError::TlsProtocol), 0),
            "an https request through a seam with no trust roots must answer TlsProtocol before \
             dialing the host"
        );
    }

    // @behavior NW-022
    #[test]
    fn a_released_requests_upstream_still_sending_holds_its_place() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let port = listener.local_addr().unwrap().port();
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("127.0.0.1", port).unwrap());
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let namespace = Namespace::new(
            [&network],
            &crate::net::Backends::new(Arc::new(HostInet), Arc::new(HostInet)),
            OpenConnections::new(Some(1)),
            None,
        )
        .expect("the endpoints compile")
        .with_outbound(Arc::new(HyperOutbound::new()));
        let cred = prepare_creds(&[], None);
        let authority = format!("127.0.0.1:{port}");
        let mut request = OutgoingRequest::new("POST", Scheme::Http, &authority);
        request.path_with_query = "/".to_string();
        request.body = vec![b'b'; 32 * 1024 * 1024];
        let exchange =
            crate::net::send(&namespace, &cred, request).expect("the endpoint admits it");
        // The peer accepts and never reads, so the request's bytes stay queued.
        let (peer, _) = listener.accept().expect("the listener accepts");
        // A locked send buffer stops Linux from growing it, so the host
        // socket fills and the connection's own queue still holds bytes.
        for upstream in namespace.tcp_sockets().held() {
            upstream.lock_host_send_buffer(4096);
        }
        thread::sleep(Duration::from_millis(200));
        exchange.release();
        let while_sending = crate::net::send(&namespace, &cred, greeting_request(&authority)).err();
        drop(peer);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let once_closed = loop {
            match crate::net::send(&namespace, &cred, greeting_request(&authority)) {
                Ok(_) => break true,
                Err(_) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(1))
                }
                Err(_) => break false,
            }
        };
        assert_eq!(
            (while_sending, once_closed),
            (Some(HttpError::ConnectionLimitReached), true),
            "a request past an open_connection_limit of 1 must answer ConnectionLimitReached \
             while a released request's connection still sends its queued body, and send once \
             that connection closed"
        );
    }

    /// A host refusing every dial, whose resolver records the thread each
    /// query starts on and never answers.
    #[derive(Default)]
    struct RecordsResolveThread(Mutex<Vec<thread::ThreadId>>);

    impl ProtocolFamily for RecordsResolveThread {
        fn connect(&self, _addr: SocketAddr) -> io::Result<std::net::TcpStream> {
            Err(io::ErrorKind::ConnectionRefused.into())
        }
    }

    impl crate::net::Resolver for RecordsResolveThread {
        fn resolve(&self, _name: &str) -> crate::net::Resolving {
            self.0.lock().unwrap().push(thread::current().id());
            Box::pin(std::future::pending())
        }
    }

    // The reactor never makes the call a Resolver may block on: a request's
    // lookup starts on the thread that sends it.
    #[test]
    fn a_requests_lookup_starts_on_the_thread_that_sends_it() {
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("api.example.com", 80).unwrap());
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let family = Arc::new(RecordsResolveThread::default());
        let namespace = Namespace::new(
            [&network],
            &crate::net::Backends::new(family.clone(), family.clone()),
            OpenConnections::new(None),
            None,
        )
        .expect("the endpoints compile");
        let cred = prepare_creds(&[], None);
        let exchange = crate::net::send(&namespace, &cred, greeting_request("api.example.com"))
            .expect("the endpoint admits it");
        thread::sleep(Duration::from_millis(50));
        let started_on = family.0.lock().unwrap().clone();
        exchange.release();
        assert_eq!(
            started_on,
            vec![thread::current().id()],
            "a request's lookup must start once, on the thread that sends the request, never \
             on the reactor"
        );
    }

    // @behavior NW-085
    #[test]
    fn a_released_requests_lookup_is_cancelled_and_gives_its_place_back_at_once() {
        let (started, resolving) = std::sync::mpsc::channel();
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("api.example.com", 80).unwrap());
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let family = Arc::new(StallsLookups::new(started));
        let namespace = Namespace::new(
            [&network],
            &crate::net::Backends::new(family.clone(), family.clone()),
            OpenConnections::new(Some(1)),
            None,
        )
        .expect("the endpoints compile");
        let cred = prepare_creds(&[], None);
        let first = crate::net::send(&namespace, &cred, greeting_request("api.example.com"))
            .expect("the first request has a place");
        resolving
            .recv_timeout(Duration::from_secs(5))
            .expect("the request's lookup starts");
        first.release();
        let next = crate::net::send(&namespace, &cred, greeting_request("api.example.com"));
        assert_eq!(
            (next.is_ok(), family.cancelled()),
            (true, 1),
            "with an open_connection_limit of 1, a request sent after one released while its \
             authority still resolves must take the place at once, the released request's query \
             cancelled though the resolver never answered"
        );
    }

    // @behavior NW-022
    #[test]
    fn a_lookup_or_a_request_holding_the_last_place_makes_a_connect_answer_eagain() {
        let (started, resolving) = std::sync::mpsc::channel();
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("api.example.com", 80).unwrap());
        entry.endpoint(
            Endpoint::new("127.0.0.1", 4567)
                .unwrap()
                .with_protocol(Protocol::Tcp),
        );
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let family = Arc::new(StallsLookups::new(started));
        let namespace = Namespace::new(
            [&network],
            &crate::net::Backends::new(family.clone(), family),
            OpenConnections::new(Some(1)),
            None,
        )
        .expect("the endpoints compile");
        let cred = prepare_creds(&[], None);
        let connect = || {
            let sock =
                crate::net::TcpSock::create(&namespace, &cred, crate::net::AddressFamily::Inet)?;
            let addr = "127.0.0.1:4567".parse().ok();
            crate::net::sys_connect(&namespace, &cred, sock.as_ref(), addr)
        };
        let lookup = crate::net::resolve(&namespace, "api.example.com").expect("a lookup starts");
        resolving
            .recv_timeout(Duration::from_secs(5))
            .expect("the guest's lookup reaches the resolver");
        let past_lookup = connect().err();
        drop(lookup);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let request = loop {
            match crate::net::send(&namespace, &cred, greeting_request("api.example.com")) {
                Ok(request) => break request,
                Err(_) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(1))
                }
                Err(err) => panic!("the lookup's place never came back: {err:?}"),
            }
        };
        resolving
            .recv_timeout(Duration::from_secs(5))
            .expect("the request's lookup reaches the resolver");
        let past_request = connect().err();
        drop(request);
        assert_eq!(
            (past_lookup, past_request),
            (Some(Errno::WouldBlock), Some(Errno::WouldBlock)),
            "a socket's connect must answer EAGAIN while a guest's lookup, or a request's, holds \
             the last place of an open_connection_limit of 1"
        );
    }

    // @behavior NW-196
    #[test]
    fn a_request_dials_through_a_socket_of_the_invocations_own() {
        let (port, server) = serve_once();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut entry = Builder::default();
        entry.endpoint(
            Endpoint::new("localhost", port)
                .unwrap()
                .with_addresses(vec![crate::network::Addresses::Local]),
        );
        let recorded = seen.clone();
        entry.connect_policy(move |connection: &crate::network::Connection<'_>| {
            recorded.lock().unwrap().push(connection.host.to_string());
            true
        });
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let namespace =
            Namespace::for_network(&network, Arc::new(HostInet), Arc::new(HyperOutbound::new()));
        let cred_within = |limit| {
            crate::security::restrict_self(
                &prepare_creds(&[], limit),
                [&crate::grant::Grant::Network(network.clone())],
            )
        };
        let request = || greeting_request(&format!("localhost:{port}"));
        let cramped = crate::net::send(
            &namespace,
            &cred_within(Some(MAX_READ_BUFFER + 2 * MAX_SOCKET_BUFFER)),
            request(),
        )
        .map(|exchange| read_response(&exchange).map(|(head, _)| head.status));
        let roomy = crate::net::send(&namespace, &cred_within(None), request())
            .map(|exchange| read_response(&exchange).map(|(head, _)| head.status));
        assert_eq!(
            (cramped, roomy, seen.lock().unwrap().clone()),
            (
                Ok(Err(HttpError::Internal)),
                Ok(Ok(200)),
                vec!["localhost".to_string(), "localhost".to_string()]
            ),
            "a guest's request must fail before it reaches the server when the Invocation's \
             memory cannot hold its upstream socket's buffers, reach it with room, and show the \
             connect policy only the host the guest named, never an address it resolved to"
        );
        server.join().unwrap();
    }

    // @behavior NW-037
    #[test]
    fn a_request_to_an_http_endpoint_reads_back_the_response() {
        let (port, server) = serve_once();
        let namespace = namespace(vec![Endpoint::new("localhost", port)
            .unwrap()
            .with_addresses(vec![crate::network::Addresses::Local])]);
        let cred = prepare_creds(&[], None);
        let exchange = crate::net::send(
            &namespace,
            &cred,
            greeting_request(&format!("LocalHost:{port}")),
        )
        .expect("the endpoint names the authority");
        let (head, body) = read_response(&exchange).expect("the server answers");
        let seen = server.join().unwrap();
        assert!(
            head.status == 200
                && head
                    .headers
                    .contains(&("x-kind".to_string(), b"test".to_vec()))
                && body == b"hello"
                && seen[0] == "GET /greeting?lang=en HTTP/1.1",
            "a guest's request to an http endpoint must send the request line and read back the \
             status, headers, and body, got {head:?} {body:?} after {seen:?}"
        );
    }

    /// A seam recording the authority each request it is handed names, and
    /// answering none.
    #[derive(Default)]
    struct RecordsAuthority(Mutex<Vec<String>>);

    impl Outbound for RecordsAuthority {
        fn send_request(&self, request: OutgoingRequest, _connector: Connector) -> ResponseFuture {
            self.0.lock().unwrap().push(request.authority);
            Box::pin(async { Err(HttpError::Internal) })
        }
    }

    // @behavior NW-199
    #[test]
    fn a_request_names_its_server_as_the_endpoint_judged_it() {
        let (port, server) = serve_once();
        let namespace = namespace(vec![Endpoint::new("localhost", port)
            .unwrap()
            .with_addresses(vec![crate::network::Addresses::Local])]);
        let cred = prepare_creds(&[], None);
        let exchange = crate::net::send(
            &namespace,
            &cred,
            greeting_request(&format!("LocalHost.:{port}")),
        )
        .expect("the endpoint names the authority");
        read_response(&exchange).expect("the server answers");
        let seen = server.join().unwrap();
        let mut entry = Builder::default();
        entry.endpoint(
            Endpoint::new("localhost", 80)
                .unwrap()
                .with_addresses(vec![crate::network::Addresses::Local]),
        );
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let recorded = Arc::new(RecordsAuthority::default());
        let at_default = Namespace::for_network(&network, Arc::new(HostInet), recorded.clone());
        let _ = crate::net::send(&at_default, &cred, greeting_request("LOCALHOST:80"));
        assert_eq!(
            (
                seen.iter()
                    .find(|line| line.to_ascii_lowercase().starts_with("host:"))
                    .cloned(),
                recorded.0.lock().unwrap().clone()
            ),
            (
                Some(format!("host: localhost:{port}")),
                vec!["localhost".to_string()]
            ),
            "a guest's request to an authority spelled in another case, with a trailing dot, or \
             with its scheme's default port must name its server as the endpoint judged it: \
             lowercase, no trailing dot, and no default port"
        );
    }

    // @behavior NW-204
    #[test]
    fn a_response_head_past_the_read_buffer_answers_a_protocol_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let long = "v".repeat(MAX_FIELD_SIZE * 2);
            let _ =
                (&stream).write_all(format!("HTTP/1.1 200 OK\r\nx-long: {long}\r\n").as_bytes());
        });
        let namespace = namespace(vec![Endpoint::new("127.0.0.1", port)
            .unwrap()
            .with_addresses(vec![crate::network::Addresses::Local])]);
        let cred = prepare_creds(&[], None);
        let answer = crate::net::send(
            &namespace,
            &cred,
            greeting_request(&format!("127.0.0.1:{port}")),
        )
        .expect("the endpoint names the authority");
        let tight = prepare_creds(&[], Some(MAX_READ_BUFFER + MAX_SOCKET_BUFFER));
        let unheld = crate::net::send(
            &namespace,
            &tight,
            greeting_request(&format!("127.0.0.1:{port}")),
        )
        .err();
        assert_eq!(
            (read_response(&answer).err(), unheld),
            (Some(HttpError::ProtocolError), Some(HttpError::Internal)),
            "a response whose head runs past 128 KiB must answer a protocol error, and a request \
             whose Invocation cannot hold the seam's read buffer beside the request must fail \
             before it is sent"
        );
        server.join().unwrap();
    }

    // @behavior NW-203
    #[test]
    fn a_requests_method_is_judged_as_it_is_sent() {
        use crate::network::{Action, HttpRule};
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = seen.clone();
        let mut entry = Builder::default();
        entry
            .endpoint(
                Endpoint::new("127.0.0.1", 80)
                    .unwrap()
                    .with_addresses(vec![crate::network::Addresses::Local])
                    .with_rules(vec![
                        HttpRule::new(Action::Allow, Some("GET"), "/greeting"),
                        HttpRule::new(Action::Deny, Some("DELETE"), "/greeting"),
                        HttpRule::new(Action::Allow, Some("delete"), "/greeting"),
                    ]),
            )
            .endpoint(
                Endpoint::new("127.0.0.1", 81)
                    .unwrap()
                    .with_addresses(vec![crate::network::Addresses::Local]),
            )
            .request_policy(move |sent: &crate::network::Request<'_>| {
                recorded.lock().unwrap().push(sent.method.to_string());
                true
            });
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let namespace = Namespace::for_network(
            &network,
            Arc::new(HostInet),
            Arc::new(RecordsAuthority::default()),
        );
        let cred = prepare_creds(&[], None);
        let send = |method: &str, authority: &str| {
            let mut request = greeting_request(authority);
            request.method = method.to_string();
            crate::net::send(&namespace, &cred, request).is_ok()
        };
        let sent = [
            send("GET", "127.0.0.1"),
            send("get", "127.0.0.1"),
            send("delete", "127.0.0.1"),
            send("get", "127.0.0.1:81"),
        ];
        assert_eq!(
            (sent, seen.lock().unwrap().clone()),
            (
                [true, false, false, true],
                vec!["GET".to_string(), "get".to_string()]
            ),
            "a request by get must match no rule allowing GET, a request by delete must stay \
             refused by a rule denying DELETE, and the request policy must see get as sent"
        );
    }

    // @behavior NW-201
    #[test]
    fn an_authority_spelling_an_address_short_is_judged_and_named_as_that_address() {
        let mut entry = Builder::default();
        entry.endpoint(
            Endpoint::new("127.0.0.1", 80)
                .unwrap()
                .with_addresses(vec![crate::network::Addresses::Local]),
        );
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let recorded = Arc::new(RecordsAuthority::default());
        let namespace = Namespace::for_network(&network, Arc::new(HostInet), recorded.clone());
        let cred = prepare_creds(&[], None);
        let sent = crate::net::send(&namespace, &cred, greeting_request("127.1")).is_ok();
        assert_eq!(
            (sent, recorded.0.lock().unwrap().clone()),
            (true, vec!["127.0.0.1".to_string()]),
            "a guest's request to the authority 127.1 must pass an endpoint naming 127.0.0.1, \
             and name its server as that address"
        );
    }

    // @behavior NW-122
    #[test]
    fn an_http_connection_sends_without_nagles_delay() {
        /// The host's network, keeping a handle on each stream it dials.
        #[derive(Default)]
        struct KeepsStreams(Mutex<Vec<std::net::TcpStream>>);
        impl ProtocolFamily for KeepsStreams {
            fn connect(&self, addr: SocketAddr) -> io::Result<std::net::TcpStream> {
                let stream = HostInet.connect(addr)?;
                self.0.lock().unwrap().push(stream.try_clone()?);
                Ok(stream)
            }
        }
        impl crate::net::Resolver for KeepsStreams {
            fn resolve(&self, name: &str) -> crate::net::Resolving {
                crate::net::Resolver::resolve(&HostInet, name)
            }
        }
        let (port, server) = serve_once();
        let mut entry = Builder::default();
        entry.endpoint(
            Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_addresses(vec![crate::network::Addresses::Local]),
        );
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let family = Arc::new(KeepsStreams::default());
        let namespace =
            Namespace::for_network(&network, family.clone(), Arc::new(HyperOutbound::new()));
        let cred = prepare_creds(&[], None);
        let exchange = crate::net::send(
            &namespace,
            &cred,
            greeting_request(&format!("127.0.0.1:{port}")),
        )
        .expect("the endpoint names the authority");
        read_response(&exchange).expect("the server answers");
        server.join().unwrap();
        let nodelay = family
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|stream| stream.nodelay().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            nodelay,
            [true],
            "an HTTP request sent through the Protocol Family must turn TCP_NODELAY on for the \
             stream it dialed, as an HTTP client writes each request whole"
        );
    }

    // @behavior NW-058
    #[test]
    fn a_body_past_the_buffer_streams_as_it_is_read() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let size = MAX_SOCKET_BUFFER * 4;
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
        let cred = prepare_creds(&[], None);
        let exchange = crate::net::send(
            &namespace,
            &cred,
            greeting_request(&format!("127.0.0.1:{port}")),
        )
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
            body == expected && held <= MAX_SOCKET_BUFFER,
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
        let cred = prepare_creds(&[], None);
        let answers = [
            format!("127.0.0.1:{port}"),
            format!("elsewhere.example:{port}"),
            format!("198.18.0.0:{port}"),
        ]
        .map(|authority| crate::net::send(&namespace, &cred, greeting_request(&authority)).err());
        thread::sleep(Duration::from_millis(20));
        assert!(
            answers
                .iter()
                .all(|answer| *answer == Some(HttpError::Denied))
                && listener.accept().is_err(),
            "a guest's request to an authority only a tcp endpoint names, one no endpoint names, \
             or a virtual address must answer Denied, sending nothing, got {answers:?}"
        );
    }

    // @behavior NW-041
    #[test]
    fn a_request_carrying_a_header_the_seam_frames_is_refused_unsent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let namespace = namespace(vec![Endpoint::new("127.0.0.1", port).unwrap()]);
        let cred = prepare_creds(&[], None);
        let answers = ["Host", "content-length", "bad name"].map(|name| {
            let mut request = greeting_request(&format!("127.0.0.1:{port}"));
            request.headers.push((name.to_string(), b"x".to_vec()));
            crate::net::send(&namespace, &cred, request).err()
        });
        thread::sleep(Duration::from_millis(20));
        assert!(
            answers
                .iter()
                .all(|answer| *answer == Some(HttpError::Denied))
                && listener.accept().is_err(),
            "a guest's request carrying host, content-length, or a name that is no \
             token must answer Denied, sending nothing, got {answers:?}"
        );
    }

    // @behavior NW-184
    #[test]
    fn a_request_whose_target_is_not_in_origin_form_is_refused_unsent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let namespace = namespace(vec![Endpoint::new("127.0.0.1", port).unwrap()]);
        let cred = prepare_creds(&[], None);
        let answers = ["http://other.example/x", "*", "", "/a#b"].map(|target| {
            let mut request = greeting_request(&format!("127.0.0.1:{port}"));
            request.path_with_query = target.to_string();
            crate::net::send(&namespace, &cred, request).err()
        });
        thread::sleep(Duration::from_millis(20));
        assert!(
            answers
                .iter()
                .all(|answer| *answer == Some(HttpError::Denied))
                && listener.accept().is_err(),
            "a guest's request whose target is absolute-form, asterisk-form, empty, or \
             carries a fragment must answer Denied, sending nothing, got {answers:?}"
        );
    }

    // @behavior NW-219
    #[test]
    fn a_request_whose_method_is_not_a_token_is_refused_unsent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let namespace = namespace(vec![Endpoint::new("127.0.0.1", port).unwrap()]);
        let cred = prepare_creds(&[], None);
        let answers = ["", "GET X"].map(|method| {
            let mut request = greeting_request(&format!("127.0.0.1:{port}"));
            request.method = method.to_string();
            crate::net::send(&namespace, &cred, request).err()
        });
        thread::sleep(Duration::from_millis(20));
        assert!(
            answers
                .iter()
                .all(|answer| *answer == Some(HttpError::Denied))
                && listener.accept().is_err(),
            "a guest's request whose method is empty or holds a space must answer Denied, \
             sending nothing, got {answers:?}"
        );
    }

    // @behavior NW-048
    #[test]
    fn an_https_request_opens_tls_before_any_byte_of_http() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // The server reads the whole first record before it answers in clear,
        // and drains the client before it closes: a close with bytes unread
        // resets the connection on Windows before the client parses a byte.
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut header = [0; 5];
            stream.read_exact(&mut header).unwrap();
            let length = u16::from_be_bytes([header[3], header[4]]);
            stream
                .read_exact(&mut vec![0; usize::from(length)])
                .unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap();
            stream.shutdown(std::net::Shutdown::Write).unwrap();
            let _ = stream.read_to_end(&mut Vec::new());
            header[0]
        });
        let namespace = namespace(vec![Endpoint::new("127.0.0.1", port).unwrap()]);
        let cred = prepare_creds(&[], None);
        let mut request = greeting_request(&format!("127.0.0.1:{port}"));
        request.scheme = Scheme::Https;
        let exchange =
            crate::net::send(&namespace, &cred, request).expect("an endpoint names the authority");
        let answer = read_response(&exchange).map(|(head, _)| head.status);
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
        let namespace =
            Namespace::for_network(&network, Arc::new(HostInet), Arc::new(HyperOutbound::new()));
        let mut request = greeting_request(&format!("127.0.0.1:{port}"));
        request
            .headers
            .push(("X-Probe".to_string(), b"secret".to_vec()));
        let cred = prepare_creds(&[], None);
        let answer = crate::net::send(&namespace, &cred, request).err();
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
             header names in lower case however the request spelled them, and its refusal must \
             answer Denied, sending nothing"
        );
    }

    /// A seam recording the credentials each request it is handed carries, and
    /// answering none.
    #[derive(Default)]
    struct RecordsSwaps(Mutex<Vec<Vec<Swap>>>);

    impl Outbound for RecordsSwaps {
        fn send_request(&self, request: OutgoingRequest, _connector: Connector) -> ResponseFuture {
            self.0.lock().unwrap().push(request.swaps);
            Box::pin(async { Err(HttpError::Internal) })
        }
    }

    /// The swaps a request with `header: value` carries once its entry binds a
    /// credential to `exact.example.com` and `*.example.com` on 443, allowing
    /// GET on `/api/**`.
    fn swaps_of(scheme: Scheme, host: &str, path: &str, header: &str, value: &[u8]) -> Vec<Swap> {
        send_bound(scheme, host, path, &[(header, value)])
            .expect("an endpoint names the authority")
            .unwrap_or_default()
    }

    /// What `send` answers a request with `headers` under the entry `swaps_of`
    /// describes: the swaps the seam was handed, `None` when it was handed no
    /// request.
    fn send_bound(
        scheme: Scheme,
        host: &str,
        path: &str,
        headers: &[(&str, &[u8])],
    ) -> Result<Option<Vec<Swap>>, HttpError> {
        let mut entry = Builder::default();
        entry
            .endpoint(
                Endpoint::new("exact.example.com", 443)
                    .unwrap()
                    .with_rules(vec![crate::network::HttpRule::new(
                        crate::network::Action::Allow,
                        Some("GET"),
                        "/api/**",
                    )]),
            )
            .endpoint(Endpoint::new("*.example.com", 443).unwrap())
            .credential(crate::network::Credential::new(
                crate::network::Placement::Bearer,
                "kobako-token",
                Arc::new(|_: &crate::network::Request<'_>| {
                    Ok(Some(crate::network::Secret::new("s3cret")))
                }),
            ));
        let network = Network::new("api", Arc::new(entry)).unwrap();
        let seam = Arc::new(RecordsSwaps::default());
        let family = Arc::new(CountsDials::default());
        let namespace = Namespace::for_network(&network, family, seam.clone());
        let mut request = OutgoingRequest::new("GET", scheme, format!("{host}:443"));
        request.path_with_query = path.to_string();
        request.headers = headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_vec()))
            .collect();
        let sent = crate::net::send(&namespace, &prepare_creds(&[], None), request);
        if let Ok(exchange) = &sent {
            let _ = read_response(exchange);
        }
        let recorded = seam.0.lock().unwrap().pop();
        sent.map(|_| recorded)
    }

    // @behavior NW-220
    #[test]
    fn a_request_carrying_a_credentials_placeholder_more_than_once_is_refused_unsent() {
        let answer = |scheme, headers: &[(&str, &[u8])]| {
            send_bound(scheme, "exact.example.com", "/api/repos", headers)
                .map(|handed| handed.map(|swaps| swaps.len()))
        };
        assert_eq!(
            [
                answer(
                    Scheme::Https,
                    &[("authorization", b"Bearer kobako-token kobako-token")]
                ),
                answer(
                    Scheme::Https,
                    &[
                        ("authorization", b"Bearer kobako-token"),
                        ("Authorization", b"Bearer kobako-token"),
                    ]
                ),
                // `a:kobako-tokenkobako-token`, encoded.
                answer(
                    Scheme::Https,
                    &[(
                        "authorization",
                        b"Basic YTprb2Jha28tdG9rZW5rb2Jha28tdG9rZW4="
                    )]
                ),
                answer(
                    Scheme::Https,
                    &[
                        ("authorization", b"Bearer kobako-token"),
                        ("x-echo", b"kobako-token kobako-token"),
                    ]
                ),
            ],
            [
                Err(HttpError::Denied),
                Err(HttpError::Denied),
                Err(HttpError::Denied),
                Ok(Some(1)),
            ],
            "a request whose credential header carries the placeholder more than once, in one \
             value, across two, or inside a Basic pair, must answer Denied with nothing sent, \
             and one carrying it once must go with its one swap whatever another header holds"
        );
    }

    // @behavior NW-050
    #[test]
    fn a_credential_binds_only_to_https_its_exact_host_and_an_allowed_path() {
        let bound = |scheme, host, path, header| {
            !swaps_of(scheme, host, path, header, b"Bearer kobako-token").is_empty()
        };
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
        let masked = format!("{:?}", swap.secret);
        let mut request = OutgoingRequest::new("GET", Scheme::Https, "api.example.com:443");
        request.path_with_query = "/".to_string();
        request.headers = vec![
            ("Authorization".to_string(), b"Bearer kobako-token".to_vec()),
            ("x-echo".to_string(), b"kobako-token".to_vec()),
        ];
        request.swaps = vec![swap];
        let sent = to_hyper_request(request).expect("the request's headers are valid");
        let header = |name| {
            sent.headers()
                .get(name)
                .map(|value| value.as_bytes().to_vec())
        };
        assert_eq!(
            (
                header("authorization"),
                header("x-echo"),
                sent.headers().len(),
                masked
            ),
            (
                Some(b"Bearer s3cret".to_vec()),
                Some(b"kobako-token".to_vec()),
                3,
                "Secret(**)".to_string()
            ),
            "the default outbound seam must send a request with its credential's placeholder \
             replaced in the header the credential names only, every other header as the guest \
             set it, and the secret must print as a mask"
        );
    }

    // @behavior NW-076
    #[test]
    fn a_request_sends_its_secret_inside_a_basic_pair_carrying_the_placeholder() {
        let sent = |value: &[u8]| {
            swaps_of(
                Scheme::Https,
                "exact.example.com",
                "/api/repos",
                "authorization",
                value,
            )
            .iter()
            .fold(value.to_vec(), |value, swap| {
                swap.apply("authorization", &value)
            })
        };
        assert_eq!(
            [sent(b"Basic dXNlcjprb2Jha28tdG9rZW4="), sent(b"Basic YQ=="),],
            [b"Basic dXNlcjpzM2NyZXQ=".to_vec(), b"Basic YQ==".to_vec()],
            "a request whose Basic pair encodes the placeholder must go with the secret in the \
             pair, encoded again, as OpenShell's does, and a pair without it must go as sent"
        );
    }

    // @behavior NW-116
    #[test]
    fn a_socket_and_an_exchange_hold_their_buffers_against_the_invocations_memory() {
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("api.example.com", 80).unwrap());
        let network = Network::new("api", Arc::new(entry)).unwrap();
        let namespace = Namespace::for_network(
            &network,
            Arc::new(CountsDials::default()),
            Arc::new(RecordsSwaps::default()),
        );
        let cred = prepare_creds(&[], Some(2 * MAX_SOCKET_BUFFER));
        let tcp = crate::net::TcpSock::create(&namespace, &cred, crate::net::AddressFamily::Inet);
        let udp =
            crate::net::UdpSock::create(&namespace, &cred, crate::net::AddressFamily::Inet).err();
        let sent = crate::net::send(
            &namespace,
            &cred,
            OutgoingRequest::new("GET", Scheme::Http, "api.example.com:80"),
        )
        .err();
        let made = tcp.is_ok();
        drop(tcp);
        let again =
            crate::net::UdpSock::create(&namespace, &cred, crate::net::AddressFamily::Inet).is_ok();
        assert_eq!(
            (made, udp, sent, again),
            (
                true,
                Some(Errno::OutOfMemory),
                Some(HttpError::Internal),
                true
            ),
            "with a memory_limit holding one TCP socket's buffers, a UDP socket must answer \
             ENOMEM and a request fail before it is sent, and a UDP socket must be made once the \
             TCP socket is gone"
        );
    }

    /// A seam answering each request at once with one header of `.0` bytes and
    /// no body.
    struct AnswersHeader(usize);

    impl Outbound for AnswersHeader {
        fn send_request(&self, _: OutgoingRequest, _: Connector) -> ResponseFuture {
            let size = self.0;
            Box::pin(async move {
                Ok(Response {
                    head: ResponseHead {
                        status: 200,
                        headers: vec![("x-large".to_string(), vec![b'v'; size])],
                    },
                    body: http_body_util::Empty::<Bytes>::new()
                        .map_err(|never| match never {})
                        .boxed_unsync(),
                })
            })
        }
    }

    // @behavior NW-156
    #[test]
    fn a_response_head_past_the_invocations_memory_answers_internal_error() {
        let status = |size: usize| {
            let mut entry = Builder::default();
            entry.endpoint(Endpoint::new("api.example.com", 80).unwrap());
            let network = Network::new("api", Arc::new(entry)).unwrap();
            let namespace = Namespace::for_network(
                &network,
                Arc::new(CountsDials::default()),
                Arc::new(AnswersHeader(size)),
            );
            let cred = prepare_creds(&[], Some(MAX_READ_BUFFER + MAX_SOCKET_BUFFER + 16 * 1024));
            let exchange = crate::net::send(
                &namespace,
                &cred,
                OutgoingRequest::new("GET", Scheme::Http, "api.example.com:80"),
            )
            .expect("an endpoint names the authority");
            read_response(&exchange).map(|(head, _)| head.status)
        };
        assert_eq!(
            (status(1024), status(64 * 1024)),
            (Ok(200), Err(HttpError::Internal)),
            "a response whose head fits the Invocation's memory beside its buffer must be \
             answered, and one whose 64 KiB head does not must fail as internal"
        );
    }

    // @behavior NW-156
    #[test]
    fn a_released_response_holds_its_head_until_the_exchange_is_dropped() {
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("api.example.com", 80).unwrap());
        let network = Network::new("api", Arc::new(entry)).unwrap();
        let namespace = Namespace::for_network(
            &network,
            Arc::new(CountsDials::default()),
            Arc::new(AnswersHeader(24 * 1024)),
        );
        let cred = prepare_creds(&[], Some(MAX_READ_BUFFER + MAX_SOCKET_BUFFER + 40 * 1024));
        let exchange = crate::net::send(
            &namespace,
            &cred,
            OutgoingRequest::new("GET", Scheme::Http, "api.example.com:80"),
        )
        .expect("an endpoint names the authority");
        read_response(&exchange).expect("a 24 KiB head fits the memory");
        exchange.release();
        // Everything but the head: what the exchange gave back on release.
        let rest = MAX_READ_BUFFER + MAX_SOCKET_BUFFER + 30 * 1024;
        let while_held = MemoryCharge::new(cred.memory_budget(), rest).is_ok();
        drop(exchange);
        let after = MemoryCharge::new(cred.memory_budget(), rest).is_ok();
        assert_eq!(
            (while_held, after),
            (false, true),
            "a response whose 24 KiB head was read and its body finished must keep that head \
             against the Invocation's memory while the guest holds the response, so the \
             buffers and 30 KiB more are refused, and give it back once the response is dropped"
        );
    }

    /// A seam that never answers, holding each request it is handed.
    struct NeverAnswers;

    impl Outbound for NeverAnswers {
        fn send_request(&self, request: OutgoingRequest, _: Connector) -> ResponseFuture {
            Box::pin(async move {
                let _held = request;
                std::future::pending().await
            })
        }
    }

    /// A seam that dials the request's authority, then never answers.
    struct DialsThenWaits;

    impl Outbound for DialsThenWaits {
        fn send_request(&self, request: OutgoingRequest, connector: Connector) -> ResponseFuture {
            Box::pin(async move {
                let _held = request;
                let _connection = connector.dial().await?;
                std::future::pending().await
            })
        }
    }

    /// The answer `exchange` settles on, once it settles. Ten seconds is far
    /// past any timeout these tests set, so an exchange still unsettled then
    /// is one whose dial never completed and left no timeout running.
    fn settled(exchange: &Exchange) -> Result<ResponseHead, HttpError> {
        let give_up = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match exchange.response() {
                Some(answer) => break answer,
                None if std::time::Instant::now() < give_up => {
                    thread::sleep(Duration::from_millis(1))
                }
                None => panic!(
                    "the exchange never settled: its dial to the loopback listener did not \
                     complete, so look at the reactor driving the dial before the timeouts"
                ),
            }
        }
    }

    // @behavior NW-205
    #[test]
    fn a_request_past_its_connect_or_first_byte_timeout_answers_that_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let timed = |seam: Arc<dyn Outbound>, timeouts: Timeouts| {
            let mut entry = Builder::default();
            entry.endpoint(
                Endpoint::new("127.0.0.1", port)
                    .unwrap()
                    .with_addresses(vec![crate::network::Addresses::Local]),
            );
            let network = Network::new("web", Arc::new(entry)).unwrap();
            let namespace = Namespace::for_network(&network, Arc::new(HostInet), seam);
            let mut request = greeting_request(&format!("127.0.0.1:{port}"));
            request.timeouts = timeouts;
            crate::net::send(&namespace, &prepare_creds(&[], None), request)
                .expect("the endpoint names the authority")
        };
        let at_once = Some(Duration::ZERO);
        let unconnected = timed(
            Arc::new(NeverAnswers),
            Timeouts {
                connect: at_once,
                ..Timeouts::default()
            },
        );
        let unanswered = timed(
            Arc::new(DialsThenWaits),
            Timeouts {
                first_byte: at_once,
                ..Timeouts::default()
            },
        );
        let sent = Instant::now();
        let unset = timed(Arc::new(NeverAnswers), Timeouts::default());
        let unset_bound = unset
            .deadline()
            .map(|at| at.saturating_duration_since(sent).as_secs());
        let unset_head = timed(Arc::new(DialsThenWaits), Timeouts::default());
        let give_up = Instant::now() + Duration::from_secs(10);
        let connected = loop {
            match unset_head.connected.get() {
                Some(at) => break *at,
                None if Instant::now() < give_up => thread::sleep(Duration::from_millis(1)),
                None => panic!(
                    "the request never connected to the loopback listener, so look at the \
                     reactor driving the dial before the head's bound"
                ),
            }
        };
        let unset_head_bound = unset_head
            .deadline()
            .map(|at| at.saturating_duration_since(connected).as_secs());
        assert_eq!(
            (
                settled(&unconnected).err(),
                settled(&unanswered).err(),
                unset_bound,
                unset.response(),
                unset_head_bound,
                unset_head.response()
            ),
            (
                Some(HttpError::ConnectionTimeout),
                Some(HttpError::ConnectionReadTimeout),
                Some(600),
                None,
                Some(600),
                None
            ),
            "a request whose connect outlasts its connect timeout must answer connection-timeout, \
             one connected whose head outlasts its first-byte timeout connection-read-timeout, \
             and one setting no timeout must wait on, its connect bounded 600 seconds after it \
             was sent and its head 600 seconds after it connected"
        );
    }

    // @behavior NW-205
    #[test]
    fn a_body_whose_next_part_outlasts_its_timeout_fails_its_read() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 2 {
                    line.clear();
                }
                (&stream)
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\n")
                    .unwrap();
                // Hold the connection open with the body unsent until the
                // client lets it go.
                let _ = reader.read_line(&mut line);
            }
        });
        let namespace = namespace(vec![Endpoint::new("127.0.0.1", port)
            .unwrap()
            .with_addresses(vec![crate::network::Addresses::Local])]);
        let mut request = greeting_request(&format!("127.0.0.1:{port}"));
        request.timeouts.between_bytes = Some(Duration::ZERO);
        let cred = prepare_creds(&[], None);
        let exchange =
            crate::net::send(&namespace, &cred, request).expect("the endpoint names the authority");
        let head = settled(&exchange).map(|head| head.status);
        let mut buf = [0; 8];
        let read = exchange.read(&mut buf);
        let readable = exchange.alloc_body_file(&cred).poll(None) == PollMask::IN;
        exchange.release();
        let sent = Instant::now();
        let unset = crate::net::send(
            &namespace,
            &cred,
            greeting_request(&format!("127.0.0.1:{port}")),
        )
        .expect("the endpoint names the authority");
        let unset_head = settled(&unset).map(|head| head.status);
        let unset_bound = unset
            .deadline()
            .map(|at| at.saturating_duration_since(sent).as_secs());
        unset.release();
        server.join().unwrap();
        assert_eq!(
            (head, read, readable, unset_head, unset_bound),
            (Ok(200), Err(Errno::TimedOut), true, Ok(200), Some(600)),
            "a body whose next part outlasts its between-bytes timeout must fail its read as \
             ETIMEDOUT once its head arrived, and its file must poll readable so a waiting \
             reader meets that failure; a body setting no timeout must bound its wait 600 \
             seconds after its head"
        );
    }

    // @behavior NW-156
    #[test]
    fn a_request_in_flight_holds_its_parts_against_the_invocations_memory() {
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("api.example.com", 80).unwrap());
        let network = Network::new("api", Arc::new(entry)).unwrap();
        let namespace = Namespace::for_network(
            &network,
            Arc::new(CountsDials::default()),
            Arc::new(NeverAnswers),
        );
        let cred = prepare_creds(&[], Some(MAX_READ_BUFFER + MAX_SOCKET_BUFFER + 40 * 1024));
        let mut request = OutgoingRequest::new("GET", Scheme::Http, "api.example.com:80");
        request.path_with_query = format!("/{}", "p".repeat(30 * 1024));
        let exchange =
            crate::net::send(&namespace, &cred, request).expect("an endpoint names the authority");
        let while_sent = MemoryCharge::new(cred.memory_budget(), 20 * 1024).is_ok();
        exchange.release();
        let after = MemoryCharge::new(cred.memory_budget(), 20 * 1024).is_ok();
        assert_eq!(
            (while_sent, after),
            (false, true),
            "a request whose 30 KiB path is under way must hold it against a memory_limit of \
             its buffer and 40 KiB, so 20 KiB more is refused, and give it back once released"
        );
    }

    mod stress {
        use std::task::Context;
        use std::time::Instant;

        use super::*;

        /// A body that never ends, a frame ready at every poll.
        struct EndlessBody;

        impl Body for EndlessBody {
            type Data = Bytes;
            type Error = HttpError;

            fn poll_frame(
                self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<Option<Result<http_body::Frame<Bytes>, HttpError>>> {
                Poll::Ready(Some(Ok(http_body::Frame::data(Bytes::from_static(
                    &[0; 1024],
                )))))
            }
        }

        /// A seam answering each request at once with an endless body.
        struct StreamsEndlessly;

        impl Outbound for StreamsEndlessly {
            fn send_request(&self, _: OutgoingRequest, _: Connector) -> ResponseFuture {
                Box::pin(async {
                    Ok(Response {
                        head: ResponseHead {
                            status: 200,
                            headers: Vec::new(),
                        },
                        body: EndlessBody.boxed_unsync(),
                    })
                })
            }
        }

        // The reactor fills a body while the guest lets the exchange go, so the
        // last hold may fall in the middle of a fill. The reactor must answer a
        // request sent after every one of them.
        // @behavior NW-082
        #[test]
        #[ignore = "a stress witness; run by rake crates:stress"]
        fn an_exchange_let_go_while_its_body_fills_leaves_the_reactor_running() {
            let mut entry = Builder::default();
            entry.endpoint(Endpoint::new("api.example.com", 80).unwrap());
            let network = Network::new("api", Arc::new(entry)).unwrap();
            let namespace = Namespace::for_network(
                &network,
                Arc::new(CountsDials::default()),
                Arc::new(StreamsEndlessly),
            );
            let cred = prepare_creds(&[], None);
            let send = || {
                crate::net::send(
                    &namespace,
                    &cred,
                    OutgoingRequest::new("GET", Scheme::Http, "api.example.com:80"),
                )
                .expect("an endpoint names the authority")
            };
            for _ in 0..20_000 {
                let exchange = send();
                while exchange.response().is_none() {
                    std::hint::spin_loop();
                }
                let _ = exchange.read(&mut [0; 1]);
            }
            let last = send();
            let deadline = Instant::now() + Duration::from_secs(5);
            while last.response().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
            assert!(
                last.response().is_some(),
                "a request sent after 20,000 exchanges let go while their bodies fill must still \
                 be answered, so no exchange's release ever stops the reactor"
            );
        }
    }
}
