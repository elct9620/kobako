//! The resource table preview2 hands the guest its handles from, as
//! wasmtime's `ResourceTable` is: it numbers its own, apart from preview1's
//! descriptors, and counts each host-side producer against the open files
//! both share.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use kobako_runtime::net::{NameLookup, TcpSock, UdpSock};
use kobako_runtime::profile::Profile;
use kobako_runtime::vfs::{File, FilesystemError, OpenFiles, PollMask};
use kobako_runtime::wait::PollTable;

use super::bindings::wasi_sockets_network_0_2_12::ErrorCode;
use super::MAX_READ_SIZE_ALLOC;
use crate::wasi::MAX_TABLE_ENTRIES;

/// A file every view of it shares, as handles on one `struct file` do.
pub(crate) type SharedFile = Arc<Mutex<File>>;

/// The shared `file`, which a read or a write takes `&mut`.
pub(crate) fn lock(file: &SharedFile) -> MutexGuard<'_, File> {
    file.lock().unwrap_or_else(PoisonError::into_inner)
}

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

    /// Another view of the file, sharing the one the socket holds.
    pub(crate) fn shared_file(&self) -> SharedFile {
        self.file.clone()
    }
}

/// What an `input-stream` or an `output-stream` reads or writes: a socket's
/// file, whose socket holds what was written until the host takes it, or any
/// other file.
#[derive(Clone)]
pub(crate) enum Stream {
    Socket(SocketFile<TcpSock>),
    File(SharedFile),
}

impl Stream {
    /// Another view of the file the stream stands on, as Linux's `get_file` takes a
    /// reference to one `struct file`.
    pub(crate) fn shared_file(&self) -> SharedFile {
        match self {
            Stream::Socket(socket) => socket.shared_file(),
            Stream::File(file) => file.clone(),
        }
    }

    /// Read what the stream holds now; `WouldBlock` while it holds nothing
    /// yet, as `io.wit` makes a read non-blocking.
    pub(crate) fn read(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        lock(&self.shared_file()).read(buf)
    }

    /// Write what the stream takes now, never waiting.
    pub(crate) fn write(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        lock(&self.shared_file()).write(buf)
    }

    /// How many bytes a write takes now.
    pub(crate) fn room(&self) -> usize {
        match self {
            Stream::Socket(socket) => socket.sock.send_room(),
            // A file takes any write, so it offers what wasmtime-wasi offers
            // a file's output stream.
            Stream::File(_) => MAX_READ_SIZE_ALLOC,
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
        lock(&self.shared_file()).poll(table)
    }
}

/// A UDP socket, its file made as the socket is, and where WASI's state
/// machine has walked it.
pub(crate) struct UdpSocket {
    pub(crate) socket: SocketFile<UdpSock>,
    pub(crate) state: UdpState,
}

/// The steps of a UDP socket only WASI has: its bind starts and finishes
/// apart. Streaming once bound leaves a bind under way standing, as
/// wasmtime keeps its in-progress bind apart from its streams.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum UdpState {
    Unbound,
    Binding,
    Bound,
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

/// A stream either way, which `io.wit` ends alike: once a call fails or
/// answers `closed`, every later call answers `closed`.
pub(crate) trait EndingStream {
    /// Whether the stream reached that end.
    fn closed(&self) -> &AtomicBool;
}

pub(crate) struct InputStream {
    pub(crate) stream: Stream,
    /// The stream's end, which its TCP socket shares, so a shutdown of the
    /// receiving side closes the stream, as `tcp.wit` has it.
    closed: Arc<AtomicBool>,
}

impl InputStream {
    pub(crate) fn new(stream: Stream) -> Self {
        Self::with_closed(stream, Arc::default())
    }

    /// A stream whose end `closed` shares with the socket it reads.
    pub(crate) fn with_closed(stream: Stream, closed: Arc<AtomicBool>) -> Self {
        Self { stream, closed }
    }
}

impl EndingStream for InputStream {
    fn closed(&self) -> &AtomicBool {
        &self.closed
    }
}

pub(crate) struct OutputStream {
    pub(crate) stream: Stream,
    /// Whether a flush is under way, as `io.wit` has `check-write` answer zero
    /// until it completes; the stream's pollables read it too.
    pub(crate) flushing: Arc<AtomicBool>,
    /// What the last `check-write` permitted, which each write carrying bytes
    /// spends.
    pub(crate) permit: usize,
    /// The stream's end, which its pollables read too: a closed stream's
    /// pollable is ready at once.
    pub(crate) closed: Arc<AtomicBool>,
}

impl EndingStream for OutputStream {
    fn closed(&self) -> &AtomicBool {
        &self.closed
    }
}

impl OutputStream {
    pub(crate) fn new(stream: Stream) -> Self {
        Self {
            stream,
            flushing: Arc::default(),
            permit: 0,
            closed: Arc::default(),
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
    /// The end of the input stream a connect hands out, which a shutdown of
    /// the receiving side sets.
    pub(crate) input_closed: Arc<AtomicBool>,
}

/// The steps of a TCP socket only WASI has: its streams are handed out once
/// `finish-connect` sees the connect land.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TcpState {
    Unbound,
    Binding,
    Bound,
    Connecting,
    /// A connect that failed as it began, its error kept for `finish-connect`
    /// to answer, as wasmtime's connecting future resolves to it.
    ConnectFailed(ErrorCode),
    Connected,
    /// A connect that failed with its error, after which the socket takes no
    /// other, as `tcp.wit` closes it.
    Closed(ErrorCode),
}

impl TcpState {
    /// Whether a bind or a connect is under way, a failed connect's error
    /// not yet answered, as wasmtime keeps one `in_progress_operation`.
    pub(crate) fn is_in_progress(self) -> bool {
        matches!(
            self,
            Self::Binding | Self::Connecting | Self::ConnectFailed(_)
        )
    }

    /// Whether a socket option may be read or set now: as wasmtime's, a
    /// socket connecting answers `invalid-state`, and one whose connect
    /// failed answers its error.
    pub(crate) fn ensure_takes_options(self) -> Result<(), ErrorCode> {
        match self {
            Self::Connecting | Self::ConnectFailed(_) => Err(ErrorCode::InvalidState),
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
    /// An output stream, its flush under way, and whether it is closed: ready
    /// once the stream polls writable and no flush is still under way, or it
    /// failed or closed, as `io.wit` has a closed stream's pollable ready at
    /// once.
    Output(Stream, Arc<AtomicBool>, Arc<AtomicBool>),
    /// A TCP socket, ready whenever no connect is under way as it is polled,
    /// as wasmtime's socket pollable reads its operation in progress then,
    /// so one pollable serves the socket's whole life.
    TcpSocket(SocketFile<TcpSock>),
    /// Something that is always ready, as a bind that settles as it starts
    /// is.
    Ready,
}

impl Pollable {
    /// This pollable as a wait on `profile` holds it, beside a host-side
    /// producer when `has_producer` says so: a clock is due as the profile has
    /// it.
    pub(crate) fn for_wait(self, profile: Profile, has_producer: bool) -> Self {
        match self {
            Pollable::Clock { at } => Pollable::Clock {
                at: profile.clock_due(at, has_producer),
            },
            other => other,
        }
    }

    /// Whether it is ready, registering on `table` for a poll that will sleep.
    pub(crate) fn is_ready(&self, table: Option<&mut PollTable>) -> bool {
        match self {
            Pollable::Clock { at } => Instant::now() >= *at,
            Pollable::File(file, mask) => lock(file).poll(table).is_ready_for(*mask),
            Pollable::Output(stream, flushing, closed) => {
                let found = stream.poll(table);
                closed.load(Ordering::Acquire)
                    || found.contains(PollMask::ERR)
                    || found.contains(PollMask::HUP)
                    || (found.contains(PollMask::OUT)
                        && !OutputStream::is_flushing(stream, flushing))
            }
            Pollable::TcpSocket(socket) => {
                lock(&socket.file).poll(table);
                !socket.sock.is_connecting()
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
        matches!(
            self,
            Pollable::File(..) | Pollable::Output(..) | Pollable::TcpSocket(..)
        )
    }
}

/// One handle's resource, and the handle it is a child of, as wasmtime's
/// `ResourceTable` keeps each entry's parent.
struct Entry {
    resource: Box<dyn Any + Send>,
    parent: Option<u32>,
    children: usize,
    /// Whether it counts against the open files, as a host-side producer
    /// does and a view does not.
    counted: bool,
}

impl Entry {
    fn new(resource: Box<dyn Any + Send>, parent: Option<u32>, counted: bool) -> Self {
        Self {
            resource,
            parent,
            children: 0,
            counted,
        }
    }
}

pub(crate) struct Table {
    entries: BTreeMap<u32, Entry>,
    /// The numbers given back, the latest taken first, as wasmtime's
    /// `ResourceTable` reuses its free list.
    free: Vec<u32>,
    /// The number past every one handed out so far.
    next: u32,
    open_files: OpenFiles,
}

impl Table {
    /// A table holding nothing, counting its files against `open_files`.
    /// Numbers start at 1, as the Canonical ABI keeps 0 for no handle.
    pub(crate) fn new(open_files: OpenFiles) -> Self {
        Self {
            entries: BTreeMap::new(),
            free: Vec::new(),
            next: 1,
            open_files,
        }
    }

    /// Whether the Invocation holds as many files as it may, so a new socket
    /// answers `new-socket-limit`, as Linux's `socket()` answers EMFILE.
    pub(crate) fn is_full(&self) -> bool {
        self.open_files.is_full()
    }

    /// Hand out a handle for `resource`, a host-side producer that holds a
    /// file, as a socket, a lookup, or an exchange does; refused past the
    /// Invocation's open file limit.
    pub(crate) fn push_file<H: Handle>(&mut self, resource: H::Host) -> wasmtime::Result<H> {
        if self.open_files.open().is_err() {
            return Err(wasmtime::Error::msg(
                "the guest holds as many files as its limit allows",
            ));
        }
        self.insert(Box::new(resource), None, true)
            .inspect_err(|_| self.open_files.close())
            .map(H::new)
    }

    /// Hand out a handle for `resource`, a view that holds no file of its
    /// own; refused past `MAX_TABLE_ENTRIES`.
    pub(crate) fn push<H: Handle>(&mut self, resource: H::Host) -> wasmtime::Result<H> {
        self.insert(Box::new(resource), None, false).map(H::new)
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
        let handle = self.insert(Box::new(resource), Some(parent), false)?;
        self.entries
            .get_mut(&parent)
            .expect("the parent was just found")
            .children += 1;
        Ok(H::new(handle))
    }

    /// Number `resource` from the free list, else past every number handed
    /// out; refused past `MAX_TABLE_ENTRIES`.
    fn insert(
        &mut self,
        resource: Box<dyn Any + Send>,
        parent: Option<u32>,
        counted: bool,
    ) -> wasmtime::Result<u32> {
        if self.entries.len() >= MAX_TABLE_ENTRIES {
            return Err(wasmtime::Error::msg(
                "the guest holds as many resources as a table may",
            ));
        }
        let handle = self.free.pop().unwrap_or_else(|| {
            self.next += 1;
            self.next - 1
        });
        self.entries
            .insert(handle, Entry::new(resource, parent, counted));
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
                "the guest dropped or gave up a resource while a child of it lives",
            ));
        }
        let entry = self
            .entries
            .remove(&index)
            .expect("the entry was just found");
        self.free.push(index);
        if entry.counted {
            self.open_files.close();
        }
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
