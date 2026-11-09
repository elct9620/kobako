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

use bytes::Bytes;
use http_body::Body;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::connections::{open, OpenConnections, Place};
use super::lookup::NameLookup;
use super::reactor::{Reactor, Ready, Registration, Task};
use super::socket::{to_error, SocketMemory};
use super::{new_platform_tls, ProtocolFamily, MAX_SOCKET_BUFFER};
use crate::filesystem::anon_inode_getfile;
use crate::network::{Protocol, Ruleset, Secret};
use crate::vfs::{Cred, FMode, File, FileOperations, FilesystemError, Kiocb, OpenHow, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

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
const FORBIDDEN: [&str; 11] = [
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

/// A header list the guest builds or reads, held on its behalf against the
/// Invocation's memory, as a request's body is, so no list grows the host
/// past what the guest was allotted.
pub struct Fields {
    cred: Arc<Cred>,
    held: Mutex<Held>,
}

/// A list's entries and the room they take.
#[derive(Default)]
struct Held {
    entries: Vec<(String, Vec<u8>)>,
    room: usize,
}

/// The room `name` and `value` take in a list.
fn room_of(name: &str, value: &[u8]) -> usize {
    name.len() + value.len() + std::mem::size_of::<(String, Vec<u8>)>()
}

impl Fields {
    /// An empty list, charged to `cred` as it grows.
    pub fn new(cred: Arc<Cred>) -> Arc<Self> {
        Arc::new(Self {
            cred,
            held: Mutex::new(Held::default()),
        })
    }

    /// A list holding `entries`, as a response's headers are handed out, its
    /// room charged to `cred`; it may outgrow `MAX_FIELD_SIZE`, which only
    /// bounds what the guest appends.
    pub fn with_entries(
        cred: Arc<Cred>,
        entries: Vec<(String, Vec<u8>)>,
    ) -> Result<Arc<Self>, FieldError> {
        let room = entries
            .iter()
            .map(|(name, value)| room_of(name, value))
            .sum();
        cred.security_vm_enough_memory(room)
            .map_err(|_| FieldError::OutOfMemory)?;
        Ok(Arc::new(Self {
            cred,
            held: Mutex::new(Held { entries, room }),
        }))
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Append the header `name` with `value`, once `to_field_name` and
    /// `to_field_value` take them and the list keeps within `MAX_FIELD_SIZE`
    /// and the Invocation's memory.
    pub fn append(&self, name: &str, value: Vec<u8>) -> Result<(), FieldError> {
        let name = to_field_name(name)?;
        let value = to_field_value(value)?;
        let room = room_of(&name, &value);
        let mut held = self.held();
        if held.room + room > MAX_FIELD_SIZE {
            return Err(FieldError::TooLarge);
        }
        self.cred
            .security_vm_enough_memory(room)
            .map_err(|_| FieldError::OutOfMemory)?;
        held.entries.push((name, value));
        held.room += room;
        Ok(())
    }

    /// Every entry, in the order appended.
    pub fn entries(&self) -> Vec<(String, Vec<u8>)> {
        self.held().entries.clone()
    }
}

impl Drop for Fields {
    fn drop(&mut self) {
        let room = self
            .held
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .room;
        self.cred.vm_unacct_memory(room);
    }
}

/// Whether `text` is an HTTP token, as a method or a header name must be.
pub fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

/// The header name a guest may set, lowercase: a token, and none the seam
/// writes itself.
pub fn to_field_name(name: &str) -> Result<String, FieldError> {
    if !is_token(name) {
        return Err(FieldError::InvalidSyntax);
    }
    let name = name.to_ascii_lowercase();
    match FORBIDDEN.contains(&name.as_str()) {
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

/// `path` as the request target sent in origin form, as wasmtime takes one:
/// `""` is `/` and a query alone sits on `/`; anything else not starting with
/// `/`, or no URI path holds, is none.
pub fn to_origin_form(path: &str) -> Option<String> {
    let target = match path.chars().next() {
        None => "/".to_string(),
        Some('?') => format!("/{path}"),
        Some('/') => path.to_string(),
        Some(_) => return None,
    };
    hyper::http::uri::PathAndQuery::try_from(target.as_str()).ok()?;
    Some(target)
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

/// The scheme an `authorization` header names a basic pair by.
const BASIC: &[u8] = b"Basic ";

/// RFC 4648's standard alphabet, which a basic pair is encoded in.
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(bytes: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, byte)| n | u32::from(*byte) << (16 - 8 * i));
        for i in 0..4 {
            encoded.push(match i <= chunk.len() {
                true => BASE64[(n >> (18 - 6 * i) & 63) as usize],
                false => b'=',
            });
        }
    }
    encoded
}

/// The bytes `text` encodes, `None` for text that is no padded base64.
fn decode_base64(text: &[u8]) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let mut decoded = Vec::with_capacity(text.len() / 4 * 3);
    for chunk in text.chunks(4) {
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 {
            return None;
        }
        let mut n = 0u32;
        for (i, byte) in chunk[..4 - padding].iter().enumerate() {
            let value = BASE64.iter().position(|b| b == byte)? as u32;
            n |= value << (18 - 6 * i);
        }
        decoded.extend(n.to_be_bytes()[1..4 - padding].iter());
    }
    Some(decoded)
}

/// One placeholder to replace with a secret in one named header's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Swap {
    pub(crate) header: String,
    pub(crate) placeholder: Vec<u8>,
    pub(crate) secret: Secret,
}

impl Swap {
    /// `value`, the named header's, with each placeholder replaced. A `Basic`
    /// credential carries its pair encoded, so the placeholder is replaced in
    /// the decoded pair, which is encoded again, as OpenShell's does.
    fn apply(&self, name: &str, value: &[u8]) -> Vec<u8> {
        if !name.eq_ignore_ascii_case(&self.header) || self.placeholder.is_empty() {
            return value.to_vec();
        }
        if let Some(pair) = basic_pair_of(value).filter(|pair| contains(pair, &self.placeholder)) {
            let mut swapped = BASIC.to_vec();
            swapped.extend(encode_base64(&self.replace(&pair)));
            return swapped;
        }
        self.replace(value)
    }

    /// `value` with each placeholder replaced by the secret.
    fn replace(&self, value: &[u8]) -> Vec<u8> {
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

/// Whether a header's `value` carries `placeholder`, as sent or inside the
/// pair a `Basic` value encodes, as OpenShell finds one.
pub(crate) fn carries(value: &[u8], placeholder: &[u8]) -> bool {
    !placeholder.is_empty()
        && (contains(value, placeholder)
            || basic_pair_of(value).is_some_and(|pair| contains(&pair, placeholder)))
}

/// The pair a `Basic` value encodes.
fn basic_pair_of(value: &[u8]) -> Option<Vec<u8>> {
    value
        .get(..BASIC.len())
        .filter(|scheme| scheme.eq_ignore_ascii_case(BASIC))
        .and_then(|_| decode_base64(&value[BASIC.len()..]))
}

fn contains(value: &[u8], part: &[u8]) -> bool {
    value.windows(part.len()).any(|window| window == part)
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
    fn write(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        self.cred.security_vm_enough_memory(buf.len())?;
        self.bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    /// Take what was written, given back to the Invocation's memory, as the
    /// exchange that sends it reserves it again.
    pub fn take(&self) -> Vec<u8> {
        let bytes = std::mem::take(&mut *self.bytes.lock().unwrap_or_else(PoisonError::into_inner));
        self.cred.vm_unacct_memory(bytes.len());
        bytes
    }
}

/// What answers for a request body's file.
struct RequestBodyFile(Arc<RequestBody>);

impl FileOperations for RequestBodyFile {
    fn write_iter(&mut self, _iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        self.0.write(buf)
    }

    fn poll(&self, _mode: FMode, _table: Option<&mut PollTable>) -> PollMask {
        PollMask::OUT
    }

    fn is_stream(&self) -> bool {
        true
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
pub type ResponseFuture = Pin<Box<dyn Future<Output = Result<Response, HttpError>> + Send>>;

/// The replaceable HTTP sender, as wasmtime's `send_request`. It dials only
/// through the connector it is handed; one dialing any other way skips every
/// rule, which is its builder's own choice. The future runs on kobako's
/// reactor, so it must never block.
pub trait Outbound: Send + Sync {
    /// Send `request` through `connector`.
    fn send_request(&self, request: OutgoingRequest, connector: Connector) -> ResponseFuture;
}

/// The only dialer the outbound seam has: it reaches the one authority the
/// request named, resolving a name on the host and dialing each address the
/// ruleset lets it lead to, exactly, in turn.
pub struct Connector {
    host: String,
    port: u16,
    ruleset: Arc<Ruleset>,
    family: Arc<dyn ProtocolFamily>,
    /// The request's place, which a lookup of the authority holds too.
    place: Option<Arc<Place>>,
}

impl Connector {
    /// The host and port the request named.
    pub fn authority(&self) -> (&str, u16) {
        (&self.host, self.port)
    }

    /// A connection to the authority, once one of its addresses answers. The
    /// connector tries each admitted address in turn, as an HTTP client's
    /// connector does, while a guest's TCP socket keeps a loop of its own.
    pub async fn dial(&self) -> Result<Transport, HttpError> {
        let addrs = match self.host.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, self.port)],
            Err(_) => {
                let real = self.resolve().await?;
                let local = self.family.local_addresses();
                self.ruleset
                    .admitted_of(&self.host, &real, self.port, Protocol::Http, &local)
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
                Ok(connection) => match connection.wait_for_connect().await {
                    Ok(()) => return Ok(connection),
                    Err(err) => last = to_http_error(&err),
                },
                Err(err) => last = err,
            }
        }
        Err(last)
    }

    /// The host's answer for the authority's name, from a lookup holding the
    /// request's own place, as a guest's lookup holds one, so its thread
    /// counts against the caps until the resolver returns.
    async fn resolve(&self) -> Result<Vec<IpAddr>, HttpError> {
        let place = self.place.clone().ok_or(HttpError::Internal)?;
        let lookup = NameLookup::start_holding(self.family.clone(), self.host.clone(), place)
            .map_err(|_| HttpError::ConnectionLimitReached)?;
        poll_fn(|cx| lookup.poll_answer(cx))
            .await
            .map_err(|_| HttpError::DestinationNotFound)
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
    fn on_event(&self) {
        for waiter in [&self.read, &self.write] {
            if let Some(waker) = waiter.lock().unwrap_or_else(PoisonError::into_inner).take() {
                waker.wake();
            }
        }
    }
}

impl Transport {
    /// The connection `stream` begins, sent without Nagle's delay, as an HTTP
    /// client writes each request whole; a guest's own socket keeps Linux's
    /// default.
    fn new(stream: std::net::TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
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

    /// Wait until the connect under way settles, as `SO_ERROR` does and
    /// Linux's `inet_wait_for_connect` waits for.
    async fn wait_for_connect(&self) -> io::Result<()> {
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
            place: None,
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
    state: Arc<Mutex<State>>,
    wait: Arc<WaitQueueHead>,
    task: Mutex<Option<Arc<Task>>>,
    /// The request's place against its Sandbox's cap and the process's.
    place: Mutex<Option<Arc<Place>>>,
    /// What the request's body and the response's buffer hold, reserved
    /// against the Invocation's memory until the exchange is released.
    memory: Mutex<Option<SocketMemory>>,
}

impl Exchange {
    /// Send `request` through `seam` on the reactor, dialing through
    /// `connector`; it counts against `open_connections` and the process until
    /// released, and a full count answers `ConnectionLimitReached`. Its body
    /// and the response's buffer count against `cred`'s memory until then.
    pub(crate) fn start(
        seam: Arc<dyn Outbound>,
        request: OutgoingRequest,
        mut connector: Connector,
        open_connections: &Arc<OpenConnections>,
        cred: &Arc<Cred>,
    ) -> Result<Arc<Self>, HttpError> {
        let memory = SocketMemory::charge(cred, request.body.len() + MAX_SOCKET_BUFFER)
            .map_err(|_| HttpError::Internal)?;
        let place = Arc::new(open(open_connections).ok_or(HttpError::ConnectionLimitReached)?);
        connector.place = Some(place.clone());
        let place = Some(place);
        let reactor = Reactor::current().map_err(|_| HttpError::Internal)?;
        let exchange = Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                phase: Phase::Sending,
                body: VecDeque::new(),
                body_end: false,
                body_error: false,
                filler: None,
            })),
            wait: Arc::default(),
            task: Mutex::new(None),
            place: Mutex::new(place),
            memory: Mutex::new(Some(memory)),
        });
        // The task holds what it answers into, never the exchange, so the
        // exchange's last hold, which cancels the task, is never the task's.
        let (state, wait) = (exchange.state.clone(), exchange.wait.clone());
        let sending = seam.send_request(request, connector);
        let task = reactor.spawn(async move {
            match sending.await {
                Ok(response) => {
                    lock(&state).phase = Phase::Answered(response.head);
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

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
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
    /// while it holds nothing yet, and `ConnectionReset` for a body cut short.
    fn read(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
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
        lock(&self.state).body.clear();
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
    fn read_iter(&mut self, _iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        self.0.read(buf)
    }

    fn poll(&self, _mode: FMode, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.0.wait, table);
        let state = self.0.state();
        let settled = !matches!(state.phase, Phase::Sending);
        match settled && (!state.body.is_empty() || state.body_end || state.body_error) {
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
                Some(Err(_)) => lock(state).body_error = true,
                None => lock(state).body_end = true,
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
        lock(state).body.extend(&moved[..]);
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
                    let name = rustls::pki_types::ServerName::try_from(
                        connector.authority().0.to_string(),
                    )
                    .map_err(|_| HttpError::ProtocolError)?;
                    let session = tokio_rustls::TlsConnector::from(config)
                        .connect(name, connection)
                        .await
                        .map_err(|err| to_http_error(&err))?;
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
    let (mut sender, connection) = hyper::client::conn::http1::handshake(wire)
        .await
        .map_err(|err| to_hyper_error(&err))?;
    let mut connection: ConnectionTask = Some(Box::pin(async move {
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
    use std::thread;
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

    /// A host whose resolver says it started, then answers only once released.
    struct Stalled {
        started: Mutex<std::sync::mpsc::Sender<()>>,
        released: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl ProtocolFamily for Stalled {
        fn connect(&self, _addr: SocketAddr) -> io::Result<std::net::TcpStream> {
            Err(io::ErrorKind::ConnectionRefused.into())
        }

        fn resolve(&self, _name: &str) -> io::Result<Vec<IpAddr>> {
            let _ = self.started.lock().unwrap().send(());
            let _ = self.released.lock().unwrap().recv();
            Ok(Vec::new())
        }
    }

    /// A host counting each dial it refuses.
    #[derive(Default)]
    struct Counting(std::sync::atomic::AtomicUsize);

    impl ProtocolFamily for Counting {
        fn connect(&self, _addr: SocketAddr) -> io::Result<std::net::TcpStream> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(io::ErrorKind::ConnectionRefused.into())
        }
    }

    #[test]
    fn an_https_request_without_trust_roots_dials_nothing() {
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("127.0.0.1", 443).unwrap());
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let family = Arc::new(Counting::default());
        let namespace = Namespace::new(
            [&network],
            family.clone(),
            Arc::new(HyperOutbound { tls: None }),
            OpenConnections::new(None),
            None,
        )
        .expect("the endpoints compile");
        let cred = prepare_creds_within(&[], None);
        let mut request = OutgoingRequest::new("GET", Scheme::Https, "127.0.0.1:443");
        request.path_with_query = "/".to_string();
        let exchange = namespace
            .send(&cred, request)
            .expect("the endpoint admits it");
        let answer = loop {
            match exchange.response() {
                Some(answer) => break answer,
                None => thread::sleep(Duration::from_millis(1)),
            }
        };
        assert_eq!(
            (
                answer.err(),
                family.0.load(std::sync::atomic::Ordering::SeqCst)
            ),
            (Some(HttpError::TlsProtocol), 0),
            "an https request through a seam with no trust roots must answer TlsProtocol before \
             dialing the host"
        );
    }

    // @behavior NW-085
    #[test]
    fn a_requests_lookup_holds_its_place_until_the_resolver_returns() {
        let (release, released) = std::sync::mpsc::channel();
        let (started, resolving) = std::sync::mpsc::channel();
        let mut entry = Builder::default();
        entry.endpoint(Endpoint::new("api.example.com", 80).unwrap());
        let network = Network::new("web", Arc::new(entry)).unwrap();
        let family = Stalled {
            started: Mutex::new(started),
            released: Mutex::new(released),
        };
        let namespace = Namespace::new(
            [&network],
            Arc::new(family),
            Arc::new(HyperOutbound::new()),
            OpenConnections::new(Some(1)),
            None,
        )
        .expect("the endpoints compile");
        let cred = prepare_creds_within(&[], None);
        let first = namespace
            .send(&cred, get("api.example.com"))
            .expect("the first request has a place");
        resolving
            .recv_timeout(Duration::from_secs(5))
            .expect("the request's lookup starts");
        first.release();
        let while_resolving = namespace.send(&cred, get("api.example.com")).err();
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let once_returned = loop {
            match namespace.send(&cred, get("api.example.com")) {
                Ok(_) => break true,
                Err(_) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(1))
                }
                Err(_) => break false,
            }
        };
        assert_eq!(
            (while_resolving, once_returned),
            (Some(HttpError::ConnectionLimitReached), true),
            "Namespace::send past an open_connection_limit of 1 must answer \
             ConnectionLimitReached while a released request's lookup still resolves, and send \
             once the resolver returned"
        );
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

    // @behavior NW-041
    #[test]
    fn a_request_carrying_a_header_the_seam_frames_is_refused_unsent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let namespace = namespace(vec![Endpoint::new("127.0.0.1", port).unwrap()]);
        let cred = prepare_creds_within(&[], None);
        let answers = ["Host", "content-length", "bad name"].map(|name| {
            let mut request = get(&format!("127.0.0.1:{port}"));
            request.headers.push((name.to_string(), b"x".to_vec()));
            namespace.send(&cred, request).err()
        });
        thread::sleep(Duration::from_millis(20));
        assert!(
            answers
                .iter()
                .all(|answer| *answer == Some(HttpError::Denied))
                && listener.accept().is_err(),
            "Namespace::send of a request carrying host, content-length, or a name that is no \
             token must answer Denied, sending nothing, got {answers:?}"
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
        fn send_request(&self, request: OutgoingRequest, _connector: Connector) -> ResponseFuture {
            self.0.lock().unwrap().push(request.swaps);
            Box::pin(async { Err(HttpError::Internal) })
        }
    }

    /// The swaps a request with `header: value` carries once its entry binds a
    /// credential to `exact.example.com` and `*.example.com` on 443, allowing
    /// GET on `/api/**`.
    fn swaps_for(scheme: Scheme, host: &str, path: &str, header: &str, value: &[u8]) -> Vec<Swap> {
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
        request.headers.push((header.to_string(), value.to_vec()));
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
        let bound = |scheme, host, path, header| {
            !swaps_for(scheme, host, path, header, b"Bearer kobako-token").is_empty()
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
            "a credential must replace its placeholder in the header it names only, and its \
             secret must print as a mask"
        );
    }

    // @behavior NW-076
    #[test]
    fn a_request_sends_its_secret_inside_a_basic_pair_carrying_the_placeholder() {
        let sent = |value: &[u8]| {
            swaps_for(
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
        let namespace = Namespace::new(
            [&network],
            Arc::new(Resolving),
            Arc::new(Recording::default()),
            OpenConnections::new(None),
            None,
        )
        .unwrap();
        let cred = prepare_creds_within(&[], Some(2 * MAX_SOCKET_BUFFER));
        let tcp = namespace.create_tcp_socket(&cred);
        let udp = namespace.create_udp_socket(&cred).err();
        let sent = namespace
            .send(
                &cred,
                OutgoingRequest::new("GET", Scheme::Http, "api.example.com:80"),
            )
            .err();
        let made = tcp.is_ok();
        drop(tcp);
        let again = namespace.create_udp_socket(&cred).is_ok();
        assert_eq!(
            (made, udp, sent, again),
            (
                true,
                Some(FilesystemError::OutOfMemory),
                Some(HttpError::Internal),
                true
            ),
            "with a memory_limit holding one TCP socket's buffers, a UDP socket must answer \
             ENOMEM and a request fail before it is sent, and a UDP socket must be made once the \
             TCP socket is gone"
        );
    }

    mod stress {
        use std::task::Context;
        use std::time::Instant;

        use super::*;

        /// A body that never ends, a frame ready at every poll.
        struct Endless;

        impl Body for Endless {
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
        struct Streaming;

        impl Outbound for Streaming {
            fn send_request(&self, _: OutgoingRequest, _: Connector) -> ResponseFuture {
                Box::pin(async {
                    Ok(Response {
                        head: ResponseHead {
                            status: 200,
                            headers: Vec::new(),
                        },
                        body: Endless.boxed_unsync(),
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
            let namespace = Namespace::new(
                [&network],
                Arc::new(Resolving),
                Arc::new(Streaming),
                OpenConnections::new(None),
                None,
            )
            .unwrap();
            let cred = prepare_creds_within(&[], None);
            let send = || {
                namespace
                    .send(
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
