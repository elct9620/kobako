//! The resource table preview2 hands the guest its handles from, as
//! wasmtime's `ResourceTable` is. Each handle takes a number from the
//! space preview1's descriptors take theirs, since a socket is a file.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use kobako_runtime::net::{NameLookup, Namespace, ProtoOps, TcpSock, UdpSock, MAX_SOCKET_BUFFER};
use kobako_runtime::profile::Profile;
use kobako_runtime::vfs::{File, FilesystemError, PollMask};
use kobako_runtime::wait::PollTable;

use super::bindings::wasi_sockets_network_0_2_12::ErrorCode;
use crate::descriptors::{lock, Numbers, SharedFile, SocketType};

/// One socket file every view of a guest socket shares, as handles on one
/// `struct file` share one `struct socket`: the protocol's half is `S`, and
/// the last view dropped releases the socket.
pub(crate) struct SocketFile<S> {
    pub(crate) sock: Arc<S>,
    file: SharedFile,
}

impl<S> Clone for SocketFile<S> {
    fn clone(&self) -> Self {
        Self {
            sock: self.sock.clone(),
            file: self.file.clone(),
        }
    }
}

impl<S> SocketFile<S> {
    pub(crate) fn new(sock: Arc<S>, file: File) -> Self {
        Self {
            sock,
            file: Arc::new(Mutex::new(file)),
        }
    }

    /// The file, which a read or a write takes `&mut`, so the views share it
    /// behind a lock.
    pub(crate) fn file(&self) -> MutexGuard<'_, File> {
        lock(&self.file)
    }

    /// Another view of the file, as `get_file` takes a reference to one.
    pub(crate) fn get_file(&self) -> SharedFile {
        self.file.clone()
    }
}

/// How many bytes `check-write` offers a stream onto a file that takes any
/// write, as wasmtime-wasi offers a file's output stream.
const MAX_FILE_WRITE: usize = MAX_SOCKET_BUFFER;

/// What an `input-stream` or an `output-stream` reads or writes: a socket's
/// file, whose socket holds what was written until the host takes it, or any
/// other file.
#[derive(Clone)]
pub(crate) enum Stream {
    Socket(SocketFile<TcpSock>),
    File(SharedFile),
}

impl Stream {
    /// The file the stream stands on, as a view of one `struct file`.
    pub(crate) fn file(&self) -> SharedFile {
        match self {
            Stream::Socket(socket) => socket.get_file(),
            Stream::File(file) => file.clone(),
        }
    }

    /// Read what the stream holds now; `WouldBlock` while it holds nothing yet.
    pub(crate) fn read(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        lock(&self.file()).read(buf)
    }

    /// Write what the stream takes now.
    pub(crate) fn write(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        lock(&self.file()).write(buf)
    }

    /// How many bytes a write takes now.
    pub(crate) fn room(&self) -> usize {
        match self {
            Stream::Socket(socket) => socket.sock.send_room(),
            Stream::File(_) => MAX_FILE_WRITE,
        }
    }

    /// Whether what was written has left for the host, so a flush is done.
    pub(crate) fn is_drained(&self) -> bool {
        match self {
            Stream::Socket(socket) => socket.sock.is_drained(),
            Stream::File(_) => true,
        }
    }

    /// What a poll finds, registering on `table` for a poll that will sleep.
    pub(crate) fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        lock(&self.file()).poll(table)
    }
}

/// A UDP socket, its file made as the socket is, and where WASI's state
/// machine has walked it.
pub(crate) struct UdpSocket {
    pub(crate) socket: SocketFile<UdpSock>,
    pub(crate) state: UdpState,
}

/// The steps of a UDP socket only WASI has: its bind starts and finishes
/// apart, and it streams once bound.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum UdpState {
    Unbound,
    Binding,
    Bound,
    Streaming,
}

impl UdpState {
    /// Whether a bind began, which binds a UDP socket at once, so wasmtime's
    /// `is_bound` reads it bound before `finish-bind`.
    pub(crate) fn is_bound(&self) -> bool {
        !matches!(self, UdpState::Unbound)
    }
}

/// A guest's handle to one kind of resource, as wasmtime's `Resource<T>`
/// names the host type `T` its index stands for. The bindings implement it
/// for each WIT resource, as `bindgen!` takes `with:`.
pub(crate) trait Handle: Copy {
    type Host: Any + Send;

    fn new(index: u32) -> Self;

    fn index(self) -> u32;
}

/// The one network a Sandbox's Grants make, as `instance-network` hands it
/// out.
pub(crate) struct Network;

pub(crate) struct InputStream(pub(crate) Stream);

pub(crate) struct OutputStream {
    pub(crate) stream: Stream,
    /// Whether a flush is under way, as `io.wit` has `check-write` answer zero
    /// until it completes; the stream's pollables read it too.
    pub(crate) flushing: Arc<AtomicBool>,
    /// What the last `check-write` permitted, which each write spends.
    pub(crate) permit: usize,
}

impl OutputStream {
    pub(crate) fn new(stream: Stream) -> Self {
        Self {
            stream,
            flushing: Arc::default(),
            permit: 0,
        }
    }

    /// Whether a flush is under way and what it flushes has not all left.
    pub(crate) fn is_flushing(stream: &Stream, flushing: &AtomicBool) -> bool {
        flushing.load(Ordering::Acquire) && !stream.is_drained()
    }
}

/// A stream's failure, as `wasi:io/error` hands one out.
pub(crate) struct Error(pub(crate) FilesystemError);

/// A lookup, the file made for it as it starts, and whether its one address
/// was handed out.
pub(crate) struct ResolveAddressStream {
    pub(crate) lookup: Arc<NameLookup>,
    pub(crate) file: SharedFile,
    pub(crate) handed: bool,
}

pub(crate) struct IncomingDatagramStream(pub(crate) SocketFile<UdpSock>);

/// An outgoing datagram stream, and how many datagrams its last `check-send`
/// permitted, as wasmtime keeps the permit a send spends.
pub(crate) struct OutgoingDatagramStream {
    pub(crate) socket: SocketFile<UdpSock>,
    pub(crate) permit: u64,
}

/// A TCP socket, its file made as the socket is, and where WASI's state
/// machine has walked it.
pub(crate) struct TcpSocket {
    pub(crate) socket: SocketFile<TcpSock>,
    pub(crate) state: TcpState,
}

/// The steps of a TCP socket only WASI has: its streams are handed out once
/// `finish-connect` sees the connect land.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TcpState {
    Unbound,
    Binding,
    Bound,
    Connecting,
    Connected,
    /// A connect that failed with its error, after which the socket takes no
    /// other, as `tcp.wit` closes it.
    Closed(ErrorCode),
}

impl TcpState {
    /// Whether a socket option may be read or set now: as wasmtime's, a
    /// socket connecting answers `invalid-state`, and one whose connect
    /// failed answers its error.
    pub(crate) fn ensure_takes_options(self) -> Result<(), ErrorCode> {
        match self {
            Self::Connecting => Err(ErrorCode::InvalidState),
            Self::Closed(code) => Err(code),
            Self::Unbound | Self::Binding | Self::Bound | Self::Connected => Ok(()),
        }
    }
}

/// What a pollable waits for.
#[derive(Clone)]
pub(crate) enum Pollable {
    /// A clock, ready once `at` passes in host time.
    Clock { at: Instant },
    /// A file a host-side producer fills, ready once its poll finds
    /// `PollMask`, or an error or a hang-up, as Linux's `poll` reports both
    /// whatever was asked.
    File(SharedFile, PollMask),
    /// An output stream, ready once `check-write` would offer a byte: the
    /// stream takes a write and no flush is still under way, or it failed,
    /// as `io.wit` has its pollable ready.
    Output(Stream, Arc<AtomicBool>),
    /// Something that is always ready, as a bind that settles as it starts
    /// is.
    Ready,
}

impl Pollable {
    /// This pollable as a wait on `profile` holds it, beside a host-side
    /// producer when `producer` says so: a clock is due as the profile has it.
    pub(crate) fn for_wait(self, profile: Profile, producer: bool) -> Self {
        match self {
            Pollable::Clock { at } => Pollable::Clock {
                at: profile.clock_due(at, producer),
            },
            other => other,
        }
    }

    /// Whether it is ready, registering on `table` for a poll that will sleep.
    pub(crate) fn is_ready(&self, table: Option<&mut PollTable>) -> bool {
        match self {
            Pollable::Clock { at } => Instant::now() >= *at,
            Pollable::File(file, mask) => lock(file).poll(table).is_ready_for(*mask),
            Pollable::Output(stream, flushing) => {
                let found = stream.poll(table);
                found.contains(PollMask::ERR)
                    || found.contains(PollMask::HUP)
                    || (found.contains(PollMask::OUT)
                        && !OutputStream::is_flushing(stream, flushing))
            }
            Pollable::Ready => true,
        }
    }

    /// The soonest this could become ready by itself.
    pub(crate) fn due(&self) -> Option<Instant> {
        match self {
            Pollable::Clock { at } => Some(*at),
            _ => None,
        }
    }

    /// Whether a host-side producer makes this ready, as a socket's peer, a
    /// resolver, or a server does.
    pub(crate) fn is_producer(&self) -> bool {
        matches!(self, Pollable::File(..) | Pollable::Output(..))
    }
}

/// One handle's resource, and the handle it is a child of, as wasmtime's
/// `ResourceTable` keeps each entry's parent.
struct Entry {
    resource: Box<dyn Any + Send>,
    parent: Option<u32>,
    children: usize,
}

impl Entry {
    fn new(resource: Box<dyn Any + Send>, parent: Option<u32>) -> Self {
        Self {
            resource,
            parent,
            children: 0,
        }
    }
}

pub(crate) struct Table {
    entries: BTreeMap<u32, Entry>,
    numbers: Numbers,
    network: Option<Namespace>,
}

impl Table {
    /// A table holding nothing, numbering from the space preview1's
    /// descriptors share, with no network until one is granted.
    pub(crate) fn new(numbers: Numbers) -> Self {
        Self {
            entries: BTreeMap::new(),
            numbers,
            network: None,
        }
    }

    /// Give the guest `network`, as an Invocation's network Grants make it.
    pub(crate) fn grant(&mut self, network: Namespace) {
        self.network = Some(network);
    }

    pub(crate) fn network(&self) -> Option<&Namespace> {
        self.network.as_ref()
    }

    /// Whether the Invocation holds as many files as it may, so a new socket
    /// answers `new-socket-limit`, as Linux's `socket()` answers EMFILE.
    pub(crate) fn is_full(&self) -> bool {
        self.numbers.lowest_unused().is_none()
    }

    /// Hand out a handle for `resource`, a host-side producer that holds a
    /// file, as a socket, a lookup, or an exchange does; refused past the
    /// Invocation's open file limit.
    pub(crate) fn push_file<H: Handle>(&mut self, resource: H::Host) -> wasmtime::Result<H> {
        let handle = self.numbers.lowest_unused().ok_or_else(|| {
            wasmtime::Error::msg("the guest holds as many files as its limit allows")
        })?;
        self.numbers.take(handle);
        self.entries
            .insert(handle, Entry::new(Box::new(resource), None));
        Ok(H::new(handle))
    }

    /// Hand out a handle for `resource`, a view that holds no file of its
    /// own; refused past `MAX_NUMBERS`.
    pub(crate) fn push<H: Handle>(&mut self, resource: H::Host) -> wasmtime::Result<H> {
        self.insert(Box::new(resource), None).map(H::new)
    }

    /// Hand out a handle for `resource` as a child of `parent`, which cannot
    /// be dropped while it lives, as the WIT has a parent's drop trap.
    pub(crate) fn push_child<H: Handle>(
        &mut self,
        resource: H::Host,
        parent: impl Handle,
    ) -> wasmtime::Result<H> {
        let parent = parent.index();
        self.entry(parent)?;
        let handle = self.insert(Box::new(resource), Some(parent))?;
        self.entries
            .get_mut(&parent)
            .expect("the parent was just found")
            .children += 1;
        Ok(H::new(handle))
    }

    fn insert(
        &mut self,
        resource: Box<dyn Any + Send>,
        parent: Option<u32>,
    ) -> wasmtime::Result<u32> {
        let handle = self.numbers.lowest_free().ok_or_else(|| {
            wasmtime::Error::msg("the guest holds as many resources as a table may")
        })?;
        self.numbers.take_view(handle);
        self.entries.insert(handle, Entry::new(resource, parent));
        Ok(handle)
    }

    /// Whether `parent` still has a child of kind `H`, as wasmtime's
    /// `iter_children` answers.
    pub(crate) fn has_child<H: Handle>(&self, parent: impl Handle) -> bool {
        let parent = Some(parent.index());
        self.entries
            .values()
            .any(|entry| entry.parent == parent && entry.resource.is::<H::Host>())
    }

    fn entry(&self, handle: u32) -> wasmtime::Result<&Entry> {
        self.entries
            .get(&handle)
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle it does not hold"))
    }

    /// The resource `handle` stands for. A handle of another kind traps, as
    /// the Canonical ABI traps on a handle of the wrong type.
    pub(crate) fn get<H: Handle>(&self, handle: H) -> wasmtime::Result<&H::Host> {
        self.entry(handle.index())?
            .resource
            .downcast_ref()
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle of another kind"))
    }

    pub(crate) fn get_mut<H: Handle>(&mut self, handle: H) -> wasmtime::Result<&mut H::Host> {
        self.entries
            .get_mut(&handle.index())
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle it does not hold"))?
            .resource
            .downcast_mut()
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle of another kind"))
    }

    /// Have the socket's `handle` stand for its file, so a preview1 call on
    /// the number reaches the socket, as one fdtable holds both.
    pub(crate) fn install<S: ProtoOps + 'static>(
        &self,
        handle: impl Handle,
        socket: &SocketFile<S>,
        kind: SocketType,
    ) {
        let ops: Arc<dyn ProtoOps> = socket.sock.clone();
        self.numbers
            .install(handle.index(), socket.file.clone(), ops, kind);
    }

    /// Take `handle` out of the table. A handle of another kind, or one whose
    /// children still live, traps, as the Canonical ABI traps on a handle of
    /// the wrong type and the WIT on a parent dropped first.
    pub(crate) fn delete<H: Handle>(&mut self, handle: H) -> wasmtime::Result<H::Host> {
        let index = handle.index();
        let entry = self.entry(index)?;
        if !entry.resource.is::<H::Host>() {
            return Err(wasmtime::Error::msg(
                "the guest named a handle of another kind",
            ));
        }
        if entry.children > 0 {
            return Err(wasmtime::Error::msg(
                "the guest dropped a resource while a child of it lives",
            ));
        }
        let entry = self
            .entries
            .remove(&index)
            .expect("the entry was just found");
        self.numbers.give_back(index);
        if let Some(parent) = entry
            .parent
            .and_then(|parent| self.entries.get_mut(&parent))
        {
            parent.children -= 1;
        }
        Ok(*entry
            .resource
            .downcast()
            .expect("the entry was just found to be of this kind"))
    }
}
