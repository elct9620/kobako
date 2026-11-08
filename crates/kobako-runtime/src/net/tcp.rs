//! A TCP socket the guest holds, as Linux's `tcp_sock` behind a socket file.
//! The host socket is non-blocking and the reactor watches it: each change
//! moves what fits between the host socket and the socket's buffers, as a
//! softirq fills `sk_receive_queue`, then wakes the socket's wait queue. A
//! guest call moves what it can at once, so a buffer the guest drained reads
//! on without waiting for another event.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super::connections::{open, OpenConnections, Place};
use super::family::ProtocolFamily;
use super::reactor::{Reactor, Ready, Registration};
use super::socket::{to_error, ProtoOps};
use crate::vfs::{FilesystemError, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// How much a socket holds each way before it stops reading and refuses
/// more, as `sk_rcvbuf` and `sk_sndbuf` bound a Linux socket, so a full buffer
/// pushes back through the TCP window.
pub const MAX_SOCKET_BUFFER: usize = 64 * 1024;

/// How much one host read moves at once.
const CHUNK: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    /// Connected to nothing yet, as a fresh socket stands in `TCP_CLOSE`.
    Unconnected,
    Connecting,
    Connected,
    Failed(FilesystemError),
}

struct State {
    phase: Phase,
    /// The host socket, and its place in the reactor.
    host: Option<(mio::net::TcpStream, Registration)>,
    /// The addresses still to try, should the one connecting fail.
    untried: VecDeque<SocketAddr>,
    received: VecDeque<u8>,
    /// The peer ended its side, as a read of zero answers.
    received_end: bool,
    received_error: Option<FilesystemError>,
    sending: VecDeque<u8>,
    /// The guest shut its writing side down; the FIN leaves once the buffer
    /// drains.
    send_shut: bool,
    fin_sent: bool,
    send_error: Option<FilesystemError>,
    local: Option<SocketAddr>,
    /// The peer as the guest named it, a virtual address standing for its
    /// name, as cgroup `getpeername4` maps a real peer back.
    named: Option<SocketAddr>,
    /// Every view the guest held is gone.
    released: bool,
    /// The socket's place against its Sandbox's cap and the process's, given
    /// back as it is released.
    place: Option<Place>,
}

/// One TCP socket, shared by the socket file and the reactor.
pub struct TcpSock {
    state: Mutex<State>,
    /// What a guest's poll waits on.
    wait: Arc<WaitQueueHead>,
    family: Arc<dyn ProtocolFamily>,
    reactor: Arc<Reactor>,
    this: Weak<Self>,
}

impl TcpSock {
    /// A socket connected to nothing, which connects through `family`, as
    /// `inet_create` makes one.
    pub(crate) fn new(family: Arc<dyn ProtocolFamily>) -> Result<Arc<Self>, FilesystemError> {
        let reactor = Reactor::current().map_err(|err| to_error(&err))?;
        Ok(Arc::new_cyclic(|this| Self {
            state: Mutex::new(State {
                phase: Phase::Unconnected,
                host: None,
                untried: VecDeque::new(),
                received: VecDeque::new(),
                received_end: false,
                received_error: None,
                sending: VecDeque::new(),
                send_shut: false,
                fin_sent: false,
                send_error: None,
                local: None,
                named: None,
                released: false,
                place: None,
            }),
            wait: Arc::default(),
            family,
            reactor,
            this: this.clone(),
        }))
    }

    /// Connect to the first of `addrs` that answers, in turn, the guest having
    /// named `named`; the connect goes on without blocking, so this returns at
    /// once and a poll finds the socket writable once it settles. The socket
    /// counts against `open_connections` and the process until released, and a
    /// full count answers `EAGAIN`, never a Trap, so one Sandbox cannot end
    /// another's run.
    pub(crate) fn connect(
        &self,
        addrs: Vec<SocketAddr>,
        named: SocketAddr,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<(), FilesystemError> {
        let place = open(open_connections).ok_or(FilesystemError::WouldBlock)?;
        let mut state = self.state();
        state.place = Some(place);
        state.untried = addrs.into();
        state.named = Some(named);
        state.phase = Phase::Connecting;
        self.dial_next(&mut state, FilesystemError::InvalidInput);
        Ok(())
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Start connecting to the next address left, or fail with `last` once
    /// none is.
    fn dial_next(&self, state: &mut State, mut last: FilesystemError) {
        while let Some(addr) = state.untried.pop_front() {
            let dialed = self.family.connect(addr).and_then(|stream| {
                let mut stream = mio::net::TcpStream::from_std(stream);
                let ready: Weak<dyn Ready> = self.this.clone();
                let registration = self.reactor.register(&mut stream, ready)?;
                Ok((stream, registration))
            });
            match dialed {
                Ok(host) => {
                    state.host = Some(host);
                    return;
                }
                Err(err) => last = to_error(&err),
            }
        }
        state.phase = Phase::Failed(last);
    }

    /// Let the host socket go, deregistered from the reactor.
    fn close_host(state: &mut State) {
        if let Some((mut stream, registration)) = state.host.take() {
            registration.deregister(&mut stream);
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    /// Move what the host socket takes and holds now, without blocking.
    fn pump(&self, state: &mut State) {
        if state.phase == Phase::Connecting {
            self.settle_connect(state);
        }
        if state.phase != Phase::Connected {
            return;
        }
        let Some((stream, _)) = state.host.as_mut() else {
            return;
        };
        let mut chunk = [0; CHUNK];
        while state.received.len() < MAX_SOCKET_BUFFER
            && !state.received_end
            && state.received_error.is_none()
        {
            let room = (MAX_SOCKET_BUFFER - state.received.len()).min(CHUNK);
            match stream.read(&mut chunk[..room]) {
                Ok(0) => state.received_end = true,
                Ok(n) => state.received.extend(&chunk[..n]),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => state.received_error = Some(to_error(&err)),
            }
        }
        while !state.sending.is_empty() && state.send_error.is_none() {
            match stream.write(state.sending.as_slices().0) {
                Ok(n) => {
                    state.sending.drain(..n);
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => state.send_error = Some(to_error(&err)),
            }
        }
        if state.send_shut && state.sending.is_empty() && !state.fin_sent {
            let _ = stream.shutdown(Shutdown::Write);
            state.fin_sent = true;
        }
    }

    /// Whether the connect under way finished, as `SO_ERROR` settles a
    /// non-blocking `connect`; a failed one moves on to the next address.
    fn settle_connect(&self, state: &mut State) {
        let Some((stream, _)) = state.host.as_ref() else {
            return;
        };
        let failure = match (stream.take_error(), stream.peer_addr()) {
            (Ok(Some(err)), _) | (Err(err), _) => err,
            (Ok(None), Ok(_)) => {
                state.local = stream.local_addr().ok();
                state.phase = Phase::Connected;
                return;
            }
            (Ok(None), Err(err)) if err.kind() == io::ErrorKind::NotConnected => return,
            (Ok(None), Err(err)) => err,
        };
        Self::close_host(state);
        self.dial_next(state, to_error(&failure));
    }

    /// Move what can move, then wake every poller, since what a poll finds
    /// may have changed.
    fn pump_and_wake(&self, mut state: MutexGuard<'_, State>) {
        if !state.released {
            self.pump(&mut state);
        }
        drop(state);
        self.wait.wake_up();
    }

    /// How the connect ended: still under way answers EAGAIN, as a
    /// non-blocking `connect` answers `EINPROGRESS` until `SO_ERROR` settles.
    pub fn finish_connect(&self) -> Result<(), FilesystemError> {
        match &self.state().phase {
            Phase::Unconnected => Err(FilesystemError::NotConnected),
            Phase::Connecting => Err(FilesystemError::WouldBlock),
            Phase::Connected => Ok(()),
            Phase::Failed(err) => Err(err.clone()),
        }
    }

    /// How many bytes a send would take now.
    pub fn send_room(&self) -> usize {
        MAX_SOCKET_BUFFER - self.state().sending.len()
    }

    /// Whether everything sent has left for the host, as Linux's `SIOCOUTQ`
    /// reads zero.
    pub fn is_drained(&self) -> bool {
        self.state().sending.is_empty()
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
        self.pump_and_wake(state);
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.state().local
    }

    /// The peer as the guest named it, once connected, as cgroup
    /// `getpeername4` maps the real peer back.
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        let state = self.state();
        match state.phase {
            Phase::Connected => state.named,
            _ => None,
        }
    }

    /// Let go of the host socket once every view is gone, as `sock_close`
    /// runs on the last `fput`.
    fn release(&self) {
        let mut state = self.state();
        state.released = true;
        state.untried.clear();
        state.place = None;
        Self::close_host(&mut state);
        drop(state);
        self.wait.wake_up();
    }
}

impl Ready for TcpSock {
    fn ready(&self) {
        let state = self.state();
        self.pump_and_wake(state);
    }
}

/// TCP's answers for its socket file, as `inet_stream_ops` hands each call to
/// `tcp_prot`.
impl ProtoOps for TcpSock {
    /// What the receive buffer holds, up to `buf`'s length; a drained buffer
    /// answers EAGAIN until the peer ends, and the end reads as zero.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        if !state.received.is_empty() {
            let take = state.received.len().min(buf.len());
            for (slot, byte) in buf.iter_mut().zip(state.received.drain(..take)) {
                *slot = byte;
            }
            self.pump_and_wake(state);
            return Ok(take);
        }
        if let Some(err) = &state.received_error {
            return Err(err.clone());
        }
        match (&state.phase, state.received_end) {
            (_, true) => Ok(0),
            (Phase::Unconnected | Phase::Failed(_), false) => Err(FilesystemError::NotConnected),
            _ => Err(FilesystemError::WouldBlock),
        }
    }

    /// Queue what fits of `buf`, sent as the host socket takes it; a full
    /// buffer answers EAGAIN, and a shut-down writing side EPIPE.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        if let Some(err) = &state.send_error {
            return Err(err.clone());
        }
        if state.send_shut {
            return Err(FilesystemError::BrokenPipe);
        }
        match &state.phase {
            Phase::Connected => {}
            Phase::Unconnected => return Err(FilesystemError::BrokenPipe),
            Phase::Connecting => return Err(FilesystemError::WouldBlock),
            Phase::Failed(_) => return Err(FilesystemError::NotConnected),
        }
        let take = buf.len().min(MAX_SOCKET_BUFFER - state.sending.len());
        if take == 0 && !buf.is_empty() {
            return Err(FilesystemError::WouldBlock);
        }
        state.sending.extend(&buf[..take]);
        self.pump_and_wake(state);
        Ok(take)
    }

    /// As `tcp_poll` answers.
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.wait, table);
        let state = self.state();
        let mut mask = PollMask::default();
        if !state.received.is_empty() || state.received_end || state.received_error.is_some() {
            mask = mask | PollMask::IN;
        }
        if state.phase != Phase::Connecting
            && (state.sending.len() < MAX_SOCKET_BUFFER || state.send_error.is_some())
        {
            mask = mask | PollMask::OUT;
        }
        if matches!(state.phase, Phase::Failed(_))
            || state.send_error.is_some()
            || state.received_error.is_some()
        {
            mask = mask | PollMask::ERR;
        }
        if (state.received_end && state.send_shut) || state.phase == Phase::Unconnected {
            mask = mask | PollMask::HUP;
        }
        mask
    }

    fn release(&self) {
        TcpSock::release(self)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use super::*;
    use crate::net::{sock_alloc_file, HostInet};
    use crate::sched::testing::within_max_wait;
    use crate::vfs::File;
    use crate::wait::do_poll;

    /// A socket connecting to `addr` through the host.
    fn dialing(addr: SocketAddr) -> Arc<TcpSock> {
        let sock = TcpSock::new(Arc::new(HostInet)).expect("the reactor starts");
        sock.connect(vec![addr], addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        sock
    }

    /// A socket file connecting to a loopback listener, and the peer the
    /// listener accepted.
    fn connected() -> (Arc<TcpSock>, File, std::net::TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let sock = dialing(listener.local_addr().unwrap());
        let file = sock_alloc_file(sock.clone(), &Arc::default());
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
    fn a_socket_connects_without_blocking_and_its_poll_wakes() {
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

    // @behavior RT-351
    #[test]
    fn a_reset_reads_as_an_error_its_poll_reports() {
        let (_sock, mut file, peer) = connected();
        file.write(b"unread").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        // A close with bytes unread sends a reset, as Linux's `tcp_close` does.
        drop(peer);
        wait_for(&file, PollMask::ERR);
        assert_eq!(
            file.read(&mut [0; 8]),
            Err(FilesystemError::ConnectionReset),
            "a socket file's poll after the peer reset must find ERR, as tcp_poll reads sk_err, \
             and its read must answer ECONNRESET"
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
        let sock = dialing(addr);
        let file = sock_alloc_file(sock.clone(), &Arc::default());
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
