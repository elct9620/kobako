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
use super::socket::{to_error, ProtoOps, SocketMemory};
use crate::vfs::{Cred, FilesystemError, PollMask};
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
    /// Closed by a failed connect or a reset, as `tcp_done` leaves a socket
    /// in `TCP_CLOSE` with both sides shut down.
    Closed,
}

struct State {
    phase: Phase,
    /// The socket once connected, as `SS_CONNECTED`, which a later reset
    /// leaves standing, so it never connects again.
    connected: bool,
    /// The host socket, and its place in the reactor.
    host: Option<(mio::net::TcpStream, Registration)>,
    /// The addresses still to try, should the one connecting fail.
    untried: VecDeque<SocketAddr>,
    received: VecDeque<u8>,
    /// The peer ended its side, as a read of zero answers.
    received_end: bool,
    /// The guest shut its reading side down, as `RCV_SHUTDOWN`; what arrived
    /// still reads.
    rcv_shut: bool,
    sending: VecDeque<u8>,
    /// The guest shut its writing side down; the FIN leaves once the buffer
    /// drains.
    send_shut: bool,
    fin_sent: bool,
    /// The socket's pending failure, as `sk_err`: answered once, then
    /// cleared, as `sock_error` swaps it with zero.
    error: Option<FilesystemError>,
    /// The port the guest reads for its own end, which its namespace gave as
    /// the connect began, as a NAT's tuple pairs it with the host's.
    virtual_port: Option<u16>,
    /// The peer as the guest named it, a virtual address standing for its
    /// name, as cgroup `getpeername4` maps a real peer back.
    named: Option<SocketAddr>,
    /// Every view the guest held is gone.
    released: bool,
    /// The socket's place against its Sandbox's cap and the process's, given
    /// back as it is released.
    place: Option<Place>,
    /// What both buffers may hold, reserved against the Invocation's memory
    /// until the socket is released.
    memory: Option<SocketMemory>,
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
    /// `inet_create` makes one; its buffers are reserved against `cred`'s
    /// memory, and past it the socket answers `ENOMEM`.
    pub(crate) fn new(
        family: Arc<dyn ProtocolFamily>,
        cred: &Arc<Cred>,
    ) -> Result<Arc<Self>, FilesystemError> {
        let memory = SocketMemory::charge(cred, 2 * MAX_SOCKET_BUFFER)?;
        let reactor = Reactor::current().map_err(|err| to_error(&err))?;
        Ok(Arc::new_cyclic(|this| Self {
            state: Mutex::new(State {
                phase: Phase::Unconnected,
                connected: false,
                host: None,
                untried: VecDeque::new(),
                received: VecDeque::new(),
                received_end: false,
                rcv_shut: false,
                sending: VecDeque::new(),
                send_shut: false,
                fin_sent: false,
                error: None,
                virtual_port: None,
                named: None,
                released: false,
                place: None,
                memory: Some(memory),
            }),
            wait: Arc::default(),
            family,
            reactor,
            this: this.clone(),
        }))
    }

    /// Connect to the first of `addrs` that answers, in turn, the guest having
    /// named `named`; the connect goes on without blocking, so this returns at
    /// once and a poll finds the socket writable once it settles. A socket
    /// connecting answers `EALREADY` and one connected `EISCONN`, as
    /// `__inet_stream_connect` does. The socket counts against
    /// `open_connections` and the process until released, and a full count
    /// answers `EAGAIN`, never a Trap, so one Sandbox cannot end another's run.
    pub(crate) fn connect(
        &self,
        addrs: Vec<SocketAddr>,
        named: SocketAddr,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<(), FilesystemError> {
        {
            let mut state = self.state();
            match state.phase {
                Phase::Connecting => return Err(FilesystemError::AlreadyInProgress),
                Phase::Connected => return Err(FilesystemError::AlreadyConnected),
                Phase::Closed if state.connected => return Err(FilesystemError::AlreadyConnected),
                Phase::Closed => {
                    let err = state
                        .error
                        .take()
                        .unwrap_or(FilesystemError::ConnectionAborted);
                    Self::disconnect(&mut state);
                    return Err(err);
                }
                Phase::Unconnected => {}
            }
        }
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
        Self::close(state, last);
    }

    /// Close the socket on `err`, as `tcp_done` does after a failed connect
    /// or a reset: `TCP_CLOSE`, both sides shut down, the failure pending.
    fn close(state: &mut State, err: FilesystemError) {
        Self::close_host(state);
        state.phase = Phase::Closed;
        state.error = Some(err);
    }

    /// Return a socket whose connect failed to one never connected, as
    /// `tcp_disconnect` clears what the failed connect left.
    fn disconnect(state: &mut State) {
        state.phase = Phase::Unconnected;
        state.untried.clear();
        state.received.clear();
        state.received_end = false;
        state.rcv_shut = false;
        state.sending.clear();
        state.send_shut = false;
        state.fin_sent = false;
        state.error = None;
        state.named = None;
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
        if let Err(err) = Self::move_bytes(state) {
            Self::close(state, err);
        }
    }

    /// Read what arrived and write what is queued, until the host socket
    /// would block; a failure either way is the connection's end.
    fn move_bytes(state: &mut State) -> Result<(), FilesystemError> {
        let Some((stream, _)) = state.host.as_mut() else {
            return Ok(());
        };
        let mut chunk = [0; CHUNK];
        while state.received.len() < MAX_SOCKET_BUFFER && !state.received_end {
            let room = (MAX_SOCKET_BUFFER - state.received.len()).min(CHUNK);
            match stream.read(&mut chunk[..room]) {
                Ok(0) => state.received_end = true,
                Ok(n) => state.received.extend(&chunk[..n]),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(to_error(&err)),
            }
        }
        while !state.sending.is_empty() {
            match stream.write(state.sending.as_slices().0) {
                Ok(n) => {
                    state.sending.drain(..n);
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(to_error(&err)),
            }
        }
        if state.send_shut && state.sending.is_empty() && !state.fin_sent {
            let _ = stream.shutdown(Shutdown::Write);
            state.fin_sent = true;
        }
        Ok(())
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
                state.phase = Phase::Connected;
                state.connected = true;
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
    /// non-blocking `connect` answers `EINPROGRESS` until `SO_ERROR` settles;
    /// a failed one answers its failure, which reading `SO_ERROR` clears.
    pub fn finish_connect(&self) -> Result<(), FilesystemError> {
        let mut state = self.state();
        match &state.phase {
            Phase::Unconnected => Err(FilesystemError::NotConnected),
            Phase::Connecting => Err(FilesystemError::WouldBlock),
            Phase::Connected => Ok(()),
            Phase::Closed => Err(state.error.take().unwrap_or(FilesystemError::NotConnected)),
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

    /// Shut the writing side down once what is queued is sent, or the reading
    /// side, which still reads what arrived, as `shutdown` sets
    /// `SEND_SHUTDOWN` and `RCV_SHUTDOWN`.
    pub fn shutdown(&self, how: Shutdown) {
        let mut state = self.state();
        if matches!(how, Shutdown::Write | Shutdown::Both) {
            state.send_shut = true;
        }
        if matches!(how, Shutdown::Read | Shutdown::Both) {
            state.rcv_shut = true;
        }
        self.pump_and_wake(state);
    }

    /// The port the guest reads for its own end, once a connect began.
    pub fn virtual_port(&self) -> Option<u16> {
        self.state().virtual_port
    }

    /// Hold the port `alloc` gives as the guest's own, unless one is held.
    pub(crate) fn hold_virtual_port(&self, alloc: impl FnOnce() -> u16) {
        self.state().virtual_port.get_or_insert_with(alloc);
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
        state.received.clear();
        state.sending.clear();
        state.place = None;
        state.memory = None;
        Self::close_host(&mut state);
        drop(state);
        self.wait.wake_up();
    }
}

impl Ready for TcpSock {
    fn on_event(&self) {
        let state = self.state();
        self.pump_and_wake(state);
    }
}

/// TCP's answers for its socket file, as `inet_stream_ops` hands each call to
/// `tcp_prot`.
impl ProtoOps for TcpSock {
    /// What the receive buffer holds, up to `buf`'s length, as
    /// `tcp_recvmsg_locked` copies what is queued first. A drained buffer then
    /// answers a pending failure once, and the end as zero; it answers EAGAIN
    /// until either comes, and ENOTCONN on a socket that never connected.
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
        if let Some(err) = state.error.take() {
            return Err(err);
        }
        match state.phase {
            _ if state.received_end || state.rcv_shut => Ok(0),
            Phase::Closed => Ok(0),
            Phase::Unconnected => Err(FilesystemError::NotConnected),
            Phase::Connecting | Phase::Connected => Err(FilesystemError::WouldBlock),
        }
    }

    /// Queue what fits of `buf`, sent as the host socket takes it; a full
    /// buffer answers EAGAIN. A socket not connected answers as
    /// `sk_stream_wait_connect` does: EAGAIN while it connects, a pending
    /// failure once, then EPIPE. A connected one past its failure or its
    /// writing side's shutdown answers as `sk_stream_error`: the failure
    /// once, then EPIPE.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        match state.phase {
            Phase::Connecting => return Err(FilesystemError::WouldBlock),
            Phase::Connected if !state.send_shut && state.error.is_none() => {}
            _ => return Err(state.error.take().unwrap_or(FilesystemError::BrokenPipe)),
        }
        let take = buf.len().min(MAX_SOCKET_BUFFER - state.sending.len());
        if take == 0 && !buf.is_empty() {
            return Err(FilesystemError::WouldBlock);
        }
        state.sending.extend(&buf[..take]);
        self.pump_and_wake(state);
        Ok(take)
    }

    /// As `tcp_poll` answers: hung up in `TCP_CLOSE` or with both sides shut
    /// down, readable once the reading side is, and writable unless it
    /// connects, always so once its writing side is shut down.
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.wait, table);
        let state = self.state();
        let closed = matches!(state.phase, Phase::Unconnected | Phase::Closed);
        let rcv_shutdown = state.received_end || state.rcv_shut || state.phase == Phase::Closed;
        let send_shutdown = state.send_shut || state.phase == Phase::Closed;
        let mut mask = PollMask::default();
        if closed || (rcv_shutdown && send_shutdown) {
            mask = mask | PollMask::HUP;
        }
        if !state.received.is_empty() || rcv_shutdown {
            mask = mask | PollMask::IN;
        }
        if state.phase != Phase::Connecting
            && (send_shutdown || state.sending.len() < MAX_SOCKET_BUFFER)
        {
            mask = mask | PollMask::OUT;
        }
        if state.error.is_some() {
            mask = mask | PollMask::ERR;
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
    use crate::security::prepare_creds_within;
    use crate::vfs::File;
    use crate::wait::do_poll;

    /// A socket connecting to `addr` through the host.
    fn dialing(addr: SocketAddr) -> Arc<TcpSock> {
        let sock = TcpSock::new(Arc::new(HostInet), &prepare_creds_within(&[], None))
            .expect("the reactor starts");
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

    // @behavior RT-365
    #[test]
    fn a_guest_socket_keeps_nagles_algorithm() {
        let (sock, _file, _peer) = connected();
        let nodelay = sock
            .state()
            .host
            .as_ref()
            .map(|(stream, _)| stream.nodelay().unwrap());
        assert_eq!(
            nodelay,
            Some(false),
            "a guest's TCP socket connected through the Protocol Family must leave the stream's \
             TCP_NODELAY off, as Linux's tcp_init_sock does, since WASI gives the guest no \
             option to set it"
        );
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

    // @behavior RT-356
    #[test]
    fn a_connected_socket_asked_to_connect_again_answers_eisconn() {
        let (sock, _file, _peer) = connected();
        sock.finish_connect().expect("the socket connected");
        let addr = sock.peer_addr().expect("a connected socket has a peer");
        let (was_reset, _reset_file) = reset();
        let again = |sock: &TcpSock| sock.connect(vec![addr], addr, &OpenConnections::new(Some(1)));
        assert_eq!(
            [again(&sock), again(&was_reset)],
            [
                Err(FilesystemError::AlreadyConnected),
                Err(FilesystemError::AlreadyConnected)
            ],
            "TcpSock::connect on a connected socket, and on one connected then reset, must \
             answer EISCONN, as __inet_stream_connect does, before taking another place"
        );
    }

    // @behavior RT-364
    #[test]
    fn a_socket_whose_connect_failed_answers_the_failure_then_connects_afresh() {
        let closed = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let refused_at = closed.local_addr().unwrap();
        drop(closed);
        let sock = dialing(refused_at);
        let file = sock_alloc_file(sock.clone(), &Arc::default());
        wait_for(&file, PollMask::ERR);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let open_at = listener.local_addr().unwrap();
        let connect = || sock.connect(vec![open_at], open_at, &OpenConnections::new(None));
        let first = connect();
        let second = connect();
        listener
            .accept()
            .expect("the listener accepts the fresh connect");
        wait_for(&file, PollMask::OUT);
        assert_eq!(
            (
                first,
                second,
                sock.finish_connect(),
                file.poll(None).contains(PollMask::ERR)
            ),
            (
                Err(FilesystemError::ConnectionRefused),
                Ok(()),
                Ok(()),
                false
            ),
            "TcpSock::connect on a socket whose connect was refused must answer ECONNREFUSED \
             and dial nothing, then connect afresh with none of the failure left, as Linux \
             disconnects a failed socket"
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

    /// A socket file whose peer reset it, as a close with bytes unread does.
    fn reset() -> (Arc<TcpSock>, File) {
        let (sock, mut file, peer) = connected();
        file.write(b"unread").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        drop(peer);
        wait_for(&file, PollMask::ERR);
        (sock, file)
    }

    // @behavior RT-357
    #[test]
    fn a_reset_socket_answers_its_error_once_then_as_closed() {
        let (_sock, mut file) = reset();
        let mut buf = [0; 8];
        assert_eq!(
            (file.read(&mut buf), file.read(&mut buf), file.write(b"x")),
            (
                Err(FilesystemError::ConnectionReset),
                Ok(0),
                Err(FilesystemError::BrokenPipe)
            ),
            "File::read of a reset socket must answer ECONNRESET once and then zero, and \
             File::write then EPIPE, as sock_error clears sk_err"
        );
    }

    // @behavior RT-358
    #[test]
    fn a_reset_or_refused_socket_polls_hung_up() {
        let closed = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let addr = closed.local_addr().unwrap();
        drop(closed);
        let refused = sock_alloc_file(dialing(addr), &Arc::default());
        wait_for(&refused, PollMask::ERR);
        let every = PollMask::IN | PollMask::OUT | PollMask::ERR | PollMask::HUP;
        assert_eq!(
            (reset().1.poll(None), refused.poll(None)),
            (every, every),
            "a socket file whose peer reset it, and one whose connect was refused, must each \
             poll IN, OUT, ERR, and HUP, as tcp_poll answers TCP_CLOSE"
        );
    }

    // @behavior RT-359
    #[test]
    fn a_shut_down_reading_side_reads_what_arrived() {
        let (sock, mut file, mut peer) = connected();
        peer.write_all(b"queued").unwrap();
        wait_for(&file, PollMask::IN);
        sock.shutdown(Shutdown::Read);
        let mut buf = [0; 16];
        let first = file.read(&mut buf).map(|n| buf[..n].to_vec());
        assert_eq!(
            (first, file.read(&mut buf)),
            (Ok(b"queued".to_vec()), Ok(0)),
            "File::read after TcpSock::shutdown(Read) must still read the bytes that arrived, \
             then zero"
        );
    }

    // @behavior RT-360
    #[test]
    fn a_zero_byte_read_of_a_tcp_socket_answers_zero() {
        let (_sock, mut file, _peer) = connected();
        assert_eq!(
            file.read(&mut []),
            Ok(0),
            "File::read of zero bytes from a connected socket with nothing queued must answer \
             zero, as sock_read_iter does, not EAGAIN"
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
