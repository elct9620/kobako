//! The `wasi:io` resources: the streams a guest reads and writes, the
//! files each stands on, their errors, and the pollables a wait holds.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use kobako_runtime::net::{Exchange, TcpSock};
use kobako_runtime::profile::Profile;
use kobako_runtime::vfs::{File, PollMask};
use kobako_runtime::wait::PollTable;
use kobako_runtime::Errno;

use super::bindings::wasi_sockets_network_0_2_12::IpAddressFamily;
use super::MAX_READ_SIZE_ALLOC;

/// A file every view of it shares, as handles on one `struct file` do.
pub(crate) type SharedFile = Arc<Mutex<File>>;

/// The shared `file`, which a read or a write takes `&mut`.
pub(crate) fn lock(file: &SharedFile) -> MutexGuard<'_, File> {
    file.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One socket file every view of a guest socket shares, as handles on one
/// `struct file` share one `struct socket`: the protocol's half is `S`, and
/// the last view dropped releases the socket. It keeps the family the guest
/// made it of, as wasmtime's socket does, so an address of another family is
/// refused before the runtime is asked.
pub(crate) struct SocketFile<S> {
    pub(crate) sock: Arc<S>,
    pub(crate) family: IpAddressFamily,
    file: SharedFile,
}

impl<S> Clone for SocketFile<S> {
    fn clone(&self) -> Self {
        Self {
            sock: self.sock.clone(),
            family: self.family,
            file: self.file.clone(),
        }
    }
}

impl<S> SocketFile<S> {
    pub(crate) fn new(sock: Arc<S>, family: IpAddressFamily, file: File) -> Self {
        Self {
            sock,
            family,
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

impl From<File> for Stream {
    /// A stream over `file`, which it holds alone.
    fn from(file: File) -> Self {
        Stream::File(Arc::new(Mutex::new(file)))
    }
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

    /// The file the stream stands on, which a read or a write takes `&mut`.
    pub(crate) fn lock(&self) -> MutexGuard<'_, File> {
        match self {
            Stream::Socket(socket) => socket.file(),
            Stream::File(file) => lock(file),
        }
    }

    /// Read what the stream holds now; `WouldBlock` while it holds nothing
    /// yet, as `io.wit` makes a read non-blocking.
    pub(crate) fn read(&self, buf: &mut [u8]) -> Result<usize, Errno> {
        self.lock().read(buf)
    }

    /// Write what the stream takes now, never waiting.
    pub(crate) fn write(&self, buf: &[u8]) -> Result<usize, Errno> {
        self.lock().write(buf)
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
        self.lock().poll(table)
    }
}

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
    /// The exchange whose body the stream reads, which bounds each wait.
    pub(crate) exchange: Option<Arc<Exchange>>,
}

impl InputStream {
    pub(crate) fn new(stream: Stream) -> Self {
        Self::with_closed(stream, Arc::default())
    }

    /// A stream whose end `closed` shares with the socket it reads.
    pub(crate) fn with_closed(stream: Stream, closed: Arc<AtomicBool>) -> Self {
        Self {
            stream,
            closed,
            exchange: None,
        }
    }

    /// A stream reading `exchange`'s body.
    pub(crate) fn of_body(stream: Stream, exchange: Arc<Exchange>) -> Self {
        Self {
            exchange: Some(exchange),
            ..Self::new(stream)
        }
    }

    /// A pollable ready once the stream holds something to read, or a
    /// timeout ends the wait for it.
    pub(crate) fn pollable(&self) -> Pollable {
        let file = self.stream.shared_file();
        match &self.exchange {
            Some(exchange) => Pollable::Exchange(file, exchange.clone()),
            None => Pollable::File(file, PollMask::IN),
        }
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
pub(crate) struct Error(pub(crate) Errno);

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
    /// An exchange's response or body file, ready once it polls readable, or
    /// once a timeout the exchange holds passes, which ends its wait.
    Exchange(SharedFile, Arc<Exchange>),
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
            Pollable::Exchange(file, _) => lock(file).poll(table).is_ready_for(PollMask::IN),
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
            Pollable::Exchange(_, exchange) => exchange.deadline(),
            _ => None,
        }
    }

    /// Whether a host-side producer makes this ready, as a socket's peer, a
    /// resolver, or a server does.
    pub(crate) fn is_producer(&self) -> bool {
        matches!(
            self,
            Pollable::File(..)
                | Pollable::Exchange(..)
                | Pollable::Output(..)
                | Pollable::TcpSocket(..)
        )
    }
}
