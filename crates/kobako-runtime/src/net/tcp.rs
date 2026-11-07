//! A TCP socket the guest holds, as Linux's `tcp_sock` behind a socket file.
//! Worker threads do the blocking host I/O, as a reactor would: one connects,
//! one reads into the receive buffer, one writes the send buffer out, and each
//! wakes the socket's wait queue when what a poll finds changes. The guest's
//! side never blocks on the host, so every wait passes the wait primitive.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

use super::family::{ProtocolFamily, Stream};
use super::workers::{charge, Charge, Workers};
use crate::vfs::{
    alloc_file_pseudo, Cred, Dentry, FMode, File, FileOperations, FileSystemType, FileType,
    FilesystemError, Inode, InodeOperations, Kiocb, Kstat, OpenHow, PollMask, SuperBlock, VfsMount,
};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// How much a socket holds each way before its reader stops reading and its
/// writer refuses more, as `sk_rcvbuf` and `sk_sndbuf` bound a Linux socket,
/// so a full buffer pushes back through the TCP window.
pub const SOCKET_BUFFER: usize = 64 * 1024;

/// How much one host read or write moves at once.
const CHUNK: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Connecting,
    Connected,
    Failed(FilesystemError),
}

struct State {
    phase: Phase,
    received: VecDeque<u8>,
    /// The peer ended its side, as a read of zero answers.
    received_end: bool,
    received_error: Option<FilesystemError>,
    sending: VecDeque<u8>,
    /// The guest shut its writing side down; the writer sends its FIN once
    /// the buffer drains.
    send_shut: bool,
    send_error: Option<FilesystemError>,
    local: Option<SocketAddr>,
    peer: Option<SocketAddr>,
    /// Every view the guest held is gone.
    released: bool,
}

/// One TCP socket, shared by the socket file and its workers.
pub struct TcpSock {
    state: Mutex<State>,
    /// What the workers wait on for room or for bytes to send.
    workers: Condvar,
    /// What a guest's poll waits on.
    wait: Arc<WaitQueueHead>,
    /// A handle on the host stream, to shut down once released.
    stream: Mutex<Option<Box<dyn Stream>>>,
    deadline: Option<Instant>,
}

impl TcpSock {
    /// A socket connecting to `addr` through `family`, giving up at
    /// `deadline`; the connect runs on a worker, so this returns at once. Its
    /// two workers count against `workers` and the process, and a full count
    /// answers `EAGAIN`, never a Trap, so one Sandbox cannot end another's run.
    pub fn connect(
        family: Arc<dyn ProtocolFamily>,
        addr: SocketAddr,
        deadline: Option<Instant>,
        workers: &Arc<Workers>,
    ) -> Result<Arc<Self>, FilesystemError> {
        let mut charges = charge(workers, 2).ok_or(FilesystemError::WouldBlock)?;
        let (writer, reader) = (charges.pop(), charges.pop());
        let sock = Arc::new(Self {
            state: Mutex::new(State {
                phase: Phase::Connecting,
                received: VecDeque::new(),
                received_end: false,
                received_error: None,
                sending: VecDeque::new(),
                send_shut: false,
                send_error: None,
                local: None,
                peer: None,
                released: false,
            }),
            workers: Condvar::new(),
            wait: Arc::default(),
            stream: Mutex::new(None),
            deadline,
        });
        let worker = sock.clone();
        thread::spawn(move || {
            let _writer = writer;
            worker.connect_worker(family, addr, reader);
        });
        Ok(sock)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wake the workers and every poller, since what they wait on changed.
    fn changed(&self) {
        self.workers.notify_all();
        self.wait.wake_up();
    }

    fn time_left(&self) -> Option<std::time::Duration> {
        self.deadline
            .map(|at| at.saturating_duration_since(Instant::now()))
    }

    /// Connect, then write as the connect's worker while `reader`'s place
    /// goes to a second worker reading.
    fn connect_worker(
        self: Arc<Self>,
        family: Arc<dyn ProtocolFamily>,
        addr: SocketAddr,
        reader_charge: Option<Charge>,
    ) {
        let connected = family.connect(addr, self.time_left()).and_then(|stream| {
            let reader = stream.try_clone()?;
            let handle = stream.try_clone()?;
            Ok((stream, reader, handle))
        });
        let (writer, reader) = match connected {
            Ok((writer, reader, handle)) => {
                let mut state = self.state();
                state.local = handle.local_addr().ok();
                state.peer = handle.peer_addr().ok();
                state.phase = Phase::Connected;
                let released = state.released;
                drop(state);
                if released {
                    let _ = handle.shutdown(Shutdown::Both);
                }
                *self.stream.lock().unwrap_or_else(PoisonError::into_inner) = Some(handle);
                (writer, reader)
            }
            Err(err) => {
                self.state().phase = Phase::Failed(to_error(&err));
                self.changed();
                return;
            }
        };
        self.changed();
        let worker = self.clone();
        thread::spawn(move || {
            let _reader = reader_charge;
            worker.read_worker(reader);
        });
        self.write_worker(writer);
    }

    /// Read into the receive buffer while it has room, until the peer ends,
    /// the read fails, or the socket is released.
    fn read_worker(self: Arc<Self>, mut stream: Box<dyn Stream>) {
        let _ = stream.set_read_timeout(self.time_left());
        let mut chunk = vec![0; CHUNK];
        loop {
            let room = {
                let mut state = self.state();
                while state.received.len() >= SOCKET_BUFFER && !state.released {
                    state = self
                        .workers
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if state.released {
                    return;
                }
                SOCKET_BUFFER - state.received.len()
            };
            let read = stream.read(&mut chunk[..room.min(CHUNK)]);
            let mut state = self.state();
            let ended = match read {
                Ok(0) => {
                    state.received_end = true;
                    true
                }
                Ok(n) => {
                    state.received.extend(&chunk[..n]);
                    false
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => false,
                Err(err) => {
                    state.received_error = Some(match err.kind() {
                        io::ErrorKind::WouldBlock => FilesystemError::TimedOut,
                        _ => to_error(&err),
                    });
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

    /// Write the send buffer out as it fills, sending the FIN once a shut-down
    /// writing side drains, until the write fails or the socket is released.
    fn write_worker(self: Arc<Self>, mut stream: Box<dyn Stream>) {
        loop {
            let chunk = {
                let mut state = self.state();
                while state.sending.is_empty() && !state.send_shut && !state.released {
                    state = self
                        .workers
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if state.released {
                    return;
                }
                if state.sending.is_empty() {
                    let _ = stream.shutdown(Shutdown::Write);
                    return;
                }
                let take = state.sending.len().min(CHUNK);
                state.sending.iter().take(take).copied().collect::<Vec<_>>()
            };
            let written = stream.write_all(&chunk).and_then(|()| stream.flush());
            let mut state = self.state();
            let failed = match written {
                Ok(()) => {
                    state.sending.drain(..chunk.len());
                    false
                }
                Err(err) => {
                    state.send_error = Some(to_error(&err));
                    true
                }
            };
            drop(state);
            self.changed();
            if failed {
                return;
            }
        }
    }

    /// How the connect ended: still under way answers EAGAIN, as a
    /// non-blocking `connect` answers `EINPROGRESS` until `SO_ERROR` settles.
    pub fn finish_connect(&self) -> Result<(), FilesystemError> {
        match &self.state().phase {
            Phase::Connecting => Err(FilesystemError::WouldBlock),
            Phase::Connected => Ok(()),
            Phase::Failed(err) => Err(err.clone()),
        }
    }

    /// What the receive buffer holds, up to `buf`'s length; a drained buffer
    /// answers EAGAIN until the peer ends, and the end reads as zero.
    pub fn recv(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        if !state.received.is_empty() {
            let take = state.received.len().min(buf.len());
            for (slot, byte) in buf.iter_mut().zip(state.received.drain(..take)) {
                *slot = byte;
            }
            drop(state);
            self.changed();
            return Ok(take);
        }
        if let Some(err) = &state.received_error {
            return Err(err.clone());
        }
        match (&state.phase, state.received_end) {
            (_, true) => Ok(0),
            (Phase::Failed(_), false) => Err(FilesystemError::NotConnected),
            _ => Err(FilesystemError::WouldBlock),
        }
    }

    /// Queue what fits of `buf` for the writer; a full buffer answers EAGAIN,
    /// and a shut-down writing side EPIPE.
    pub fn send(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        if let Some(err) = &state.send_error {
            return Err(err.clone());
        }
        if state.send_shut {
            return Err(FilesystemError::BrokenPipe);
        }
        match &state.phase {
            Phase::Connected => {}
            Phase::Connecting => return Err(FilesystemError::WouldBlock),
            Phase::Failed(_) => return Err(FilesystemError::NotConnected),
        }
        let take = buf.len().min(SOCKET_BUFFER - state.sending.len());
        if take == 0 && !buf.is_empty() {
            return Err(FilesystemError::WouldBlock);
        }
        state.sending.extend(&buf[..take]);
        drop(state);
        self.changed();
        Ok(take)
    }

    /// How many bytes a send would take now.
    pub fn send_room(&self) -> usize {
        SOCKET_BUFFER - self.state().sending.len()
    }

    /// Shut the writing side down once what is queued is sent, or stop
    /// reading, as `shutdown` does.
    pub fn shutdown(&self, how: Shutdown) {
        let mut state = self.state();
        if matches!(how, Shutdown::Write | Shutdown::Both) {
            state.send_shut = true;
        }
        if matches!(how, Shutdown::Read | Shutdown::Both) {
            state.received.clear();
            state.received_end = true;
        }
        drop(state);
        self.changed();
    }

    /// What a call would find ready, as `tcp_poll` answers.
    pub fn poll_mask(&self) -> PollMask {
        let state = self.state();
        let mut mask = PollMask::default();
        if !state.received.is_empty() || state.received_end || state.received_error.is_some() {
            mask = mask | PollMask::IN;
        }
        if state.phase != Phase::Connecting
            && (state.sending.len() < SOCKET_BUFFER || state.send_error.is_some())
        {
            mask = mask | PollMask::OUT;
        }
        if matches!(state.phase, Phase::Failed(_)) || state.send_error.is_some() {
            mask = mask | PollMask::ERR;
        }
        if state.received_end && state.send_shut {
            mask = mask | PollMask::HUP;
        }
        mask
    }

    /// What a guest's poll registers on.
    pub fn wait_queue(&self) -> &Arc<WaitQueueHead> {
        &self.wait
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.state().local
    }

    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.state().peer
    }

    /// Let go of the host socket once every view is gone, as `sock_close`
    /// runs on the last `fput`: a blocked read wakes, and the workers end.
    fn release(&self) {
        self.state().released = true;
        if let Some(stream) = self
            .stream
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.changed();
    }
}

/// The POSIX name a host socket's failure stands for; one with none carries
/// only its kind, so no host address reaches the guest.
fn to_error(err: &io::Error) -> FilesystemError {
    match err.kind() {
        io::ErrorKind::ConnectionRefused => FilesystemError::ConnectionRefused,
        io::ErrorKind::ConnectionReset => FilesystemError::ConnectionReset,
        io::ErrorKind::ConnectionAborted => FilesystemError::ConnectionAborted,
        io::ErrorKind::TimedOut => FilesystemError::TimedOut,
        io::ErrorKind::HostUnreachable => FilesystemError::HostUnreachable,
        io::ErrorKind::NetworkUnreachable => FilesystemError::NetworkUnreachable,
        io::ErrorKind::AddrNotAvailable => FilesystemError::AddressNotAvailable,
        io::ErrorKind::BrokenPipe => FilesystemError::BrokenPipe,
        io::ErrorKind::NotConnected => FilesystemError::NotConnected,
        io::ErrorKind::PermissionDenied => FilesystemError::PermissionDenied,
        kind => FilesystemError::Other(format!("{kind:?}")),
    }
}

/// The one Filesystem every socket stands on, as Linux's `sock_mnt`. No path
/// reaches it: a socket is only ever open.
static SOCK_MNT: LazyLock<Arc<VfsMount>> = LazyLock::new(|| {
    let sb = SuperBlock::new(Arc::new(SockFs)).expect("sockfs fills in nothing that can fail");
    VfsMount::kern_mount(sb)
});

/// A socket reports what it is and no length, as `sock_alloc` makes one.
struct SockFs;

impl FileSystemType for SockFs {
    fn fill_super(self: Arc<Self>, sb: &mut SuperBlock) -> Result<(), FilesystemError> {
        sb.d_make_root(Inode::new(&Kstat::new(FileType::Dir, 0), self));
        Ok(())
    }
}

impl InodeOperations for SockFs {
    fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
        Ok(Kstat::new(FileType::Sock, 0))
    }
}

/// What answers for a socket file, as `socket_file_ops` hands each call to the
/// socket. The file is opened non-blocking, as a WASI 0.2 socket is, so a
/// read or write that would wait answers EAGAIN and the guest polls.
struct SocketOps(Arc<TcpSock>);

impl FileOperations for SocketOps {
    fn read_iter(&mut self, _iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        self.0.recv(buf)
    }

    fn write_iter(&mut self, _iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        self.0.send(buf)
    }

    fn poll(&self, _mode: FMode, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(self.0.wait_queue(), table);
        self.0.poll_mask()
    }

    fn is_stream(&self) -> bool {
        true
    }
}

impl Drop for SocketOps {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// The socket file of `sock`, opened as `cred` for reading and writing, as
/// Linux's `sock_alloc_file` makes one; dropping it releases the socket.
pub fn sock_alloc_file(sock: &Arc<TcpSock>, cred: &Arc<Cred>) -> File {
    let inode = Inode::new(&Kstat::new(FileType::Sock, 0), Arc::new(SockFs));
    alloc_file_pseudo(
        inode,
        &SOCK_MNT,
        &OpenHow {
            read: true,
            write: true,
            nonblock: true,
            ..OpenHow::default()
        },
        Box::new(SocketOps(sock.clone())),
        cred,
    )
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use super::*;
    use crate::net::{HostInet, SANDBOX_WORKERS};
    use crate::sched::testing::within_max_wait;
    use crate::wait::do_poll;

    /// A socket file connecting to a loopback listener, and the peer the
    /// listener accepted.
    fn connected() -> (Arc<TcpSock>, File, std::net::TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let sock = TcpSock::connect(
            Arc::new(HostInet),
            listener.local_addr().unwrap(),
            Some(Instant::now() + Duration::from_secs(10)),
            &Workers::new(SANDBOX_WORKERS),
        )
        .expect("a fresh Sandbox has room for a socket's workers");
        let file = sock_alloc_file(&sock, &Arc::default());
        let (peer, _) = listener.accept().expect("the listener accepts");
        wait_for(&file, PollMask::OUT);
        (sock, file, peer)
    }

    /// Wait on `file`'s poll until it holds `mask`.
    fn wait_for(file: &File, mask: PollMask) {
        let found = within_max_wait(|| {
            do_poll(None, |table| file.poll(table).contains(mask).then_some(()))
        });
        assert_eq!(
            found,
            Ok(Some(())),
            "the socket file's poll must find {mask:?}"
        );
    }

    // @behavior RT-342
    #[test]
    fn a_socket_connects_on_a_worker_and_its_poll_wakes() {
        let (sock, _file, _peer) = connected();
        assert_eq!(
            sock.finish_connect(),
            Ok(()),
            "TcpSock::finish_connect once the socket file's poll found it writable must succeed"
        );
    }

    // @behavior RT-343
    #[test]
    fn a_socket_reads_what_its_peer_sent_then_the_end() {
        let (_sock, mut file, mut peer) = connected();
        let mut buf = [0; 8];
        let before = file.read(&mut buf);
        peer.write_all(b"hello").unwrap();
        drop(peer);
        let mut received = Vec::new();
        loop {
            wait_for(&file, PollMask::IN);
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => received.extend_from_slice(&buf[..n]),
                Err(err) => panic!("a read the poll found ready answered {err:?}"),
            }
        }
        assert_eq!(
            (before, received),
            (Err(FilesystemError::WouldBlock), b"hello".to_vec()),
            "File::read of a socket file must answer EAGAIN before anything arrived, then the \
             peer's bytes, then zero once it closed"
        );
    }

    // @behavior RT-344
    #[test]
    fn a_socket_sends_what_it_was_handed_then_its_fin() {
        let (sock, mut file, mut peer) = connected();
        assert_eq!(
            file.write(b"ping"),
            Ok(4),
            "a connected socket takes the bytes"
        );
        sock.shutdown(Shutdown::Write);
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut received = Vec::new();
        peer.read_to_end(&mut received)
            .expect("the peer reads until the end");
        assert_eq!(
            received,
            b"ping".to_vec(),
            "a peer of a socket written then shut down for writing must read the bytes, then \
             the end"
        );
    }

    // @behavior RT-345
    #[test]
    fn a_refused_connect_answers_econnrefused() {
        let addr = TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .unwrap();
        let sock = TcpSock::connect(
            Arc::new(HostInet),
            addr,
            Some(Instant::now() + Duration::from_secs(10)),
            &Workers::new(SANDBOX_WORKERS),
        )
        .expect("a fresh Sandbox has room for a socket's workers");
        let file = sock_alloc_file(&sock, &Arc::default());
        wait_for(&file, PollMask::OUT);
        assert_eq!(
            sock.finish_connect(),
            Err(FilesystemError::ConnectionRefused),
            "TcpSock::finish_connect to a port no peer listens on must answer ECONNREFUSED"
        );
    }

    // @behavior RT-346
    #[test]
    fn dropping_the_socket_file_releases_the_host_socket() {
        let (_sock, file, mut peer) = connected();
        drop(file);
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut rest = Vec::new();
        assert!(
            peer.read_to_end(&mut rest).is_ok_and(|n| n == 0),
            "the peer of a socket whose file was dropped must read the end"
        );
    }
}
