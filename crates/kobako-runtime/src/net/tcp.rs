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
use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};

use super::connections::{open, OpenConnections};
use super::family::ProtocolFamily;
use super::reactor::{Reactor, Ready, Registration};
use super::socket::{
    ensure_within, set_hop_limit, take_connect_outcome, to_error, to_ttl, InetSock, MemoryCharge,
    ProtoOps, SockOpt, MAX_SOCKET_BUFFER,
};
use crate::vfs::{Cred, FilesystemError, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// How much one host read moves at once.
const CHUNK: usize = 16 * 1024;

/// Linux's keepalive defaults, `TCP_KEEPALIVE_TIME`, `TCP_KEEPALIVE_INTVL`,
/// and `TCP_KEEPALIVE_PROBES`, in seconds and probes.
const TCP_KEEPALIVE_TIME: u32 = 7200;
const TCP_KEEPALIVE_INTVL: u32 = 75;
const TCP_KEEPALIVE_PROBES: u32 = 9;

/// Linux's ceilings on each, past which a set answers EINVAL.
pub const MAX_TCP_KEEPIDLE: u32 = 32767;
pub const MAX_TCP_KEEPINTVL: u32 = 32767;
pub const MAX_TCP_KEEPCNT: u32 = 127;

/// When a socket probes an idle peer, as `tcp_sock` keeps `keepalive_time`,
/// `keepalive_intvl`, and `keepalive_probes` beside `SOCK_KEEPOPEN`.
#[derive(Clone, Copy)]
struct KeepAlive {
    on: bool,
    idle: u32,
    interval: u32,
    count: u32,
}

impl KeepAlive {
    /// The probes as the host socket takes them.
    fn to_params(self) -> TcpKeepalive {
        TcpKeepalive::new()
            .with_time(Duration::from_secs(self.idle.into()))
            .with_interval(Duration::from_secs(self.interval.into()))
            .with_retries(self.count)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    /// Connected to nothing yet, as a fresh socket stands in `TCP_CLOSE`.
    Unconnected,
    Connecting,
    Connected,
    /// Closed by a failed connect, as `tcp_done` leaves a socket in
    /// `TCP_CLOSE` with both sides shut down; it may connect afresh.
    Failed,
    /// Closed by a reset once connected, as `tcp_done` leaves it beside
    /// `SS_CONNECTED`, so it never connects again.
    Reset,
}

impl Phase {
    /// Whether a failed connect or a reset closed it.
    fn is_closed(&self) -> bool {
        matches!(self, Phase::Failed | Phase::Reset)
    }
}

struct State {
    phase: Phase,
    /// The host socket, and its place in the reactor.
    host: Option<(mio::net::TcpStream, Registration)>,
    /// The addresses still to try, should the one connecting fail.
    untried: VecDeque<SocketAddr>,
    /// What arrived and the guest has not read, as `sk_receive_queue`.
    receive_queue: VecDeque<u8>,
    /// The peer ended its side, as a read of zero answers.
    received_end: bool,
    /// What the guest wrote and the host has not taken, as `sk_write_queue`.
    write_queue: VecDeque<u8>,
    /// The FIN left, once the writing side was shut and the buffer drained.
    fin_sent: bool,
    /// What every inet socket holds. What arrived still reads once the
    /// reading side is shut.
    inet: InetSock,
    /// The keepalive probes the guest asked for; the host socket takes them
    /// as it is dialed.
    keepalive: KeepAlive,
    /// The socket itself once released with bytes still to send, held until
    /// they and the FIN leave, as Linux keeps an orphan in `FIN_WAIT1`.
    orphan: Option<Arc<TcpSock>>,
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
        let memory = MemoryCharge::new(cred, 2 * MAX_SOCKET_BUFFER)?;
        let reactor = Reactor::current().map_err(|err| to_error(&err))?;
        Ok(Arc::new_cyclic(|this| Self {
            state: Mutex::new(State {
                phase: Phase::Unconnected,
                host: None,
                untried: VecDeque::new(),
                receive_queue: VecDeque::new(),
                received_end: false,
                write_queue: VecDeque::new(),
                fin_sent: false,
                inet: InetSock::new(memory),
                keepalive: KeepAlive {
                    on: false,
                    idle: TCP_KEEPALIVE_TIME,
                    interval: TCP_KEEPALIVE_INTVL,
                    count: TCP_KEEPALIVE_PROBES,
                },
                orphan: None,
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
                Phase::Reset => return Err(FilesystemError::AlreadyConnected),
                Phase::Failed => {
                    let err = state
                        .inet
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
        state.inet.place = Some(place);
        state.untried = addrs.into();
        state.inet.peer = Some(named);
        state.phase = Phase::Connecting;
        self.dial_next(&mut state, FilesystemError::InvalidInput);
        Ok(())
    }

    /// Hold the port `alloc` gives as the guest's own, unless one is held,
    /// as a cgroup hook keeps what `getsockname4` answers in the socket's
    /// own storage.
    pub(crate) fn hold_virtual_port(&self, alloc: &mut dyn FnMut() -> u16) {
        self.state().inet.hold_virtual_port(alloc);
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Start connecting to the next address left, or fail with `last` once
    /// none is.
    fn dial_next(&self, state: &mut State, mut last: FilesystemError) {
        while let Some(addr) = state.untried.pop_front() {
            let dialed = self.family.connect(addr).and_then(|stream| {
                // A connect that already settled may refuse an option; its own
                // outcome is what the guest reads, as wasmtime's `apply`
                // ignores an option's failure on a socket it just made.
                let _ = Self::set_options(SockRef::from(&stream), state);
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

    /// Give the host socket `socket` the options the guest set.
    fn set_options(socket: SockRef<'_>, state: &State) -> io::Result<()> {
        socket.set_tcp_keepalive(&state.keepalive.to_params())?;
        socket.set_keepalive(state.keepalive.on)?;
        set_hop_limit(socket, state.inet.ttl)
    }

    /// Close the socket on `err`, as `tcp_done` does after a failed connect
    /// or a reset: `TCP_CLOSE`, both sides shut down, the failure pending.
    fn close(state: &mut State, err: FilesystemError) {
        Self::close_host(state);
        state.phase = match state.phase {
            Phase::Connected => Phase::Reset,
            _ => Phase::Failed,
        };
        state.inet.error = Some(err);
    }

    /// Return a socket whose connect failed or was given up to one never
    /// connected, as `tcp_disconnect` clears what the connect left.
    fn disconnect(state: &mut State) {
        state.phase = Phase::Unconnected;
        state.untried.clear();
        state.receive_queue.clear();
        state.received_end = false;
        state.inet.rcv_shut = false;
        state.write_queue.clear();
        state.inet.send_shut = false;
        state.fin_sent = false;
        state.inet.error = None;
        state.inet.peer = None;
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
        let rcvbuf = state.inet.buffers.rcvbuf;
        while state.receive_queue.len() < rcvbuf && !state.received_end {
            let room = (rcvbuf - state.receive_queue.len()).min(CHUNK);
            match stream.read(&mut chunk[..room]) {
                Ok(0) => state.received_end = true,
                Ok(n) => state.receive_queue.extend(&chunk[..n]),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(to_error(&err)),
            }
        }
        while !state.write_queue.is_empty() {
            match stream.write(state.write_queue.as_slices().0) {
                Ok(n) => {
                    state.write_queue.drain(..n);
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(to_error(&err)),
            }
        }
        if state.inet.send_shut && state.write_queue.is_empty() && !state.fin_sent {
            let _ = stream.shutdown(Shutdown::Write);
            state.fin_sent = true;
        }
        Ok(())
    }

    /// Settle the connect under way once it finished, as `SO_ERROR` settles a
    /// non-blocking `connect`: connected, or on to the next address.
    fn settle_connect(&self, state: &mut State) {
        let Some((stream, _)) = state.host.as_ref() else {
            return;
        };
        let failure = match take_connect_outcome(stream) {
            None => return,
            Some(Ok(())) => {
                state.phase = Phase::Connected;
                return;
            }
            Some(Err(err)) => err,
        };
        Self::close_host(state);
        self.dial_next(state, to_error(&failure));
    }

    /// Move what can move, then wake every poller, since what a poll finds
    /// may have changed. An orphan whose bytes and FIN left, or whose
    /// connection ended, lets go of itself; one the peer sends to resets the
    /// connection, as Linux resets data arriving after `__tcp_close`.
    fn pump_and_wake(&self, mut state: MutexGuard<'_, State>) {
        self.pump(&mut state);
        let arrived = state.orphan.is_some() && !state.receive_queue.is_empty();
        if arrived {
            Self::reset_host(&mut state);
        }
        let settled = state.orphan.is_some()
            && (arrived
                || state.phase != Phase::Connected
                || (state.write_queue.is_empty() && state.fin_sent));
        let orphan = settled.then(|| Self::let_go(&mut state)).flatten();
        drop(state);
        self.wait.wake_up();
        drop(orphan);
    }

    /// Close the host socket with a zero linger and no FIN, so its peer reads
    /// a reset.
    fn reset_host(state: &mut State) {
        if let Some((mut stream, registration)) = state.host.take() {
            registration.deregister(&mut stream);
            let _ = SockRef::from(&stream).set_linger(Some(Duration::ZERO));
        }
    }

    /// Give back the socket's place and memory and close the host socket,
    /// answering the hold an orphan kept on itself.
    fn let_go(state: &mut State) -> Option<Arc<TcpSock>> {
        state.write_queue.clear();
        state.inet.release();
        Self::close_host(state);
        state.orphan.take()
    }

    /// Let go of an orphan still sending, as the Invocation's end does with
    /// whatever its sockets still hold.
    pub(crate) fn abandon(&self) {
        let orphan = Self::let_go(&mut self.state());
        drop(orphan);
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
            Phase::Failed | Phase::Reset => Err(state
                .inet
                .error
                .take()
                .unwrap_or(FilesystemError::NotConnected)),
        }
    }

    /// How many bytes a send would take now.
    pub fn send_room(&self) -> usize {
        let state = self.state();
        state
            .inet
            .buffers
            .sndbuf
            .saturating_sub(state.write_queue.len())
    }

    /// Whether everything sent has left for the host, as Linux's `SIOCOUTQ`
    /// reads zero.
    pub fn is_drained(&self) -> bool {
        self.state().write_queue.is_empty()
    }

    /// Change what the host socket takes; a socket dialed already passes the
    /// change to its host socket at once, and keeps what it held should the
    /// host refuse, as a failed `setsockopt` changes nothing.
    fn set_options_with(&self, change: impl FnOnce(&mut State)) -> Result<(), FilesystemError> {
        let mut state = self.state();
        let kept = (state.keepalive, state.inet.ttl);
        change(&mut state);
        let Some((stream, _)) = &state.host else {
            return Ok(());
        };
        let set = Self::set_options(SockRef::from(stream), &state);
        set.map_err(|err| {
            (state.keepalive, state.inet.ttl) = kept;
            to_error(&err)
        })
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
    /// answers the peer's end as zero, ahead of any failure as `SOCK_DONE`
    /// is, then a pending failure once; it answers EAGAIN until either comes,
    /// and ENOTCONN on a socket that never connected.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        if !state.receive_queue.is_empty() {
            let take = state.receive_queue.len().min(buf.len());
            for (slot, byte) in buf.iter_mut().zip(state.receive_queue.drain(..take)) {
                *slot = byte;
            }
            self.pump_and_wake(state);
            return Ok(take);
        }
        if state.received_end {
            return Ok(0);
        }
        if let Some(err) = state.inet.error.take() {
            return Err(err);
        }
        match state.phase {
            _ if state.inet.rcv_shut => Ok(0),
            Phase::Failed | Phase::Reset => Ok(0),
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
            Phase::Connected if !state.inet.send_shut && state.inet.error.is_none() => {}
            _ => {
                return Err(state
                    .inet
                    .error
                    .take()
                    .unwrap_or(FilesystemError::BrokenPipe))
            }
        }
        let take = buf.len().min(
            state
                .inet
                .buffers
                .sndbuf
                .saturating_sub(state.write_queue.len()),
        );
        if take == 0 && !buf.is_empty() {
            return Err(FilesystemError::WouldBlock);
        }
        state.write_queue.extend(&buf[..take]);
        self.pump_and_wake(state);
        Ok(take)
    }

    /// As `tcp_poll` answers: hung up in `TCP_CLOSE` or with both sides shut
    /// down, readable once the reading side is, and writable unless it
    /// connects, always so once its writing side is shut down.
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.wait, table);
        let state = self.state();
        let closed = state.phase == Phase::Unconnected || state.phase.is_closed();
        let rcv_shutdown = state.received_end || state.inet.rcv_shut || state.phase.is_closed();
        let send_shutdown = state.inet.send_shut || state.phase.is_closed();
        let mut mask = PollMask::default();
        if closed || (rcv_shutdown && send_shutdown) {
            mask = mask | PollMask::HUP;
        }
        if !state.receive_queue.is_empty() || rcv_shutdown {
            mask = mask | PollMask::IN;
        }
        if state.phase != Phase::Connecting
            && (send_shutdown || state.write_queue.len() < state.inet.buffers.sndbuf)
        {
            mask = mask | PollMask::OUT;
        }
        if state.inet.error.is_some() {
            mask = mask | PollMask::ERR;
        }
        mask
    }

    /// Shut the writing side down once what is queued is sent, or the reading
    /// side, which still reads what arrived, as `inet_shutdown` sets
    /// `SEND_SHUTDOWN` and `RCV_SHUTDOWN`; a socket not connected marks them
    /// and answers `ENOTCONN`. One still connecting is disconnected instead,
    /// back to unconnected with nothing marked, as `inet_shutdown` disconnects
    /// one in `TCP_SYN_SENT`.
    fn shutdown(&self, how: Shutdown) -> Result<(), FilesystemError> {
        let mut state = self.state();
        if state.phase == Phase::Connecting {
            Self::close_host(&mut state);
            Self::disconnect(&mut state);
            state.inet.place = None;
            drop(state);
            self.wait.wake_up();
            return Ok(());
        }
        state.inet.mark_shutdown(how);
        let connected = state.phase == Phase::Connected;
        self.pump_and_wake(state);
        match connected {
            true => Ok(()),
            false => Err(FilesystemError::NotConnected),
        }
    }

    fn getpeername(&self) -> Option<SocketAddr> {
        let state = self.state();
        match state.phase {
            Phase::Connected => state.inet.peer,
            _ => None,
        }
    }

    fn getsockname(&self) -> SocketAddr {
        self.state().inet.getsockname()
    }

    /// Its own keepalive flag and probes, never the host's, beside the
    /// buffers and the hop limit.
    fn getsockopt(&self, opt: SockOpt) -> Result<u32, FilesystemError> {
        let state = self.state();
        match opt {
            SockOpt::KeepAlive => Ok(u32::from(state.keepalive.on)),
            SockOpt::KeepIdle => Ok(state.keepalive.idle),
            SockOpt::KeepIntvl => Ok(state.keepalive.interval),
            SockOpt::KeepCnt => Ok(state.keepalive.count),
            _ => state.inet.getsockopt(opt),
        }
    }

    /// The probes and the hop limit reach a host socket already dialed at
    /// once; each probe bound is Linux's, as `tcp_sock_set_keepidle` and its
    /// kin refuse past it with EINVAL.
    fn setsockopt(&self, opt: SockOpt, value: u32) -> Result<(), FilesystemError> {
        match opt {
            SockOpt::KeepAlive => self.set_options_with(|state| state.keepalive.on = value != 0),
            SockOpt::KeepIdle => {
                ensure_within(value, MAX_TCP_KEEPIDLE)?;
                self.set_options_with(|state| state.keepalive.idle = value)
            }
            SockOpt::KeepIntvl => {
                ensure_within(value, MAX_TCP_KEEPINTVL)?;
                self.set_options_with(|state| state.keepalive.interval = value)
            }
            SockOpt::KeepCnt => {
                ensure_within(value, MAX_TCP_KEEPCNT)?;
                self.set_options_with(|state| state.keepalive.count = value)
            }
            SockOpt::Ttl => {
                let ttl = to_ttl(value)?;
                self.set_options_with(|state| state.inet.ttl = ttl)
            }
            _ => self.state().inet.set_buffer(opt, value),
        }
    }

    /// Let go of the socket once every view is gone, as `sock_close` runs on
    /// the last `fput` and `__tcp_close` closes it: bytes still queued leave
    /// before the FIN, the socket holding itself and its place until then,
    /// and bytes that arrived unread reset the connection instead.
    fn release(&self) {
        let mut state = self.state();
        state.untried.clear();
        let unread = !std::mem::take(&mut state.receive_queue).is_empty();
        if unread {
            Self::reset_host(&mut state);
        } else if state.phase == Phase::Connected && !state.write_queue.is_empty() {
            state.inet.mark_shutdown(Shutdown::Both);
            state.orphan = self.this.upgrade();
            self.pump_and_wake(state);
            return;
        }
        let orphan = Self::let_go(&mut state);
        drop(state);
        self.wait.wake_up();
        drop(orphan);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use super::*;
    use crate::net::socket::testing::wait_for;
    use crate::net::{sock_alloc_file, HostInet};
    use crate::security::prepare_creds_within;
    use crate::vfs::File;

    /// A socket connecting to `addr` through the host.
    fn dial(addr: SocketAddr) -> Arc<TcpSock> {
        let sock = TcpSock::new(Arc::new(HostInet), &prepare_creds_within(&[], None))
            .expect("the reactor starts");
        sock.connect(vec![addr], addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        sock
    }

    /// A socket file connecting to a loopback listener, and the peer the
    /// listener accepted.
    fn connected_socket() -> (Arc<TcpSock>, File, std::net::TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let sock = dial(listener.local_addr().unwrap());
        let file = sock_alloc_file(sock.clone(), &Arc::default());
        let (peer, _) = listener.accept().expect("the listener accepts");
        wait_for(|table| file.poll(table), PollMask::OUT);
        (sock, file, peer)
    }

    // @behavior RT-381
    #[test]
    fn a_socket_file_never_waits() {
        let (_sock, mut file, _peer) = connected_socket();
        file.setfl(false, false);
        let started = std::time::Instant::now();
        let read = crate::sched::testing::within_max_wait(|| file.read(&mut [0; 8]));
        assert_eq!(
            (read, started.elapsed() < Duration::from_secs(1)),
            (Err(FilesystemError::WouldBlock), true),
            "File::read of a connected socket whose peer sent nothing must answer EAGAIN at \
             once, its non-blocking flag cleared or not, since WASI's sockets never block"
        );
    }

    // @behavior RT-365
    #[test]
    fn a_guest_socket_keeps_nagles_algorithm() {
        let (sock, _file, _peer) = connected_socket();
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

    /// Whether the host socket `sock` dialed sends keepalive probes.
    fn host_keepalive(sock: &TcpSock) -> Option<bool> {
        sock.state()
            .host
            .as_ref()
            .map(|(stream, _)| SockRef::from(stream).keepalive().unwrap())
    }

    // @behavior RT-366
    #[test]
    fn a_socket_holds_its_options_and_gives_them_to_the_host_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let addr = listener.local_addr().unwrap();
        let sock = TcpSock::new(Arc::new(HostInet), &prepare_creds_within(&[], None))
            .expect("the reactor starts");
        sock.setsockopt(SockOpt::KeepAlive, 1)
            .expect("an unconnected socket takes an option");
        let keepalive = |sock: &TcpSock| sock.getsockopt(SockOpt::KeepAlive).unwrap();
        let before = (keepalive(&sock), host_keepalive(&sock));
        sock.connect(vec![addr], addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        let dialed = host_keepalive(&sock);
        sock.setsockopt(SockOpt::KeepAlive, 0)
            .expect("a connected socket takes an option");
        let after = (keepalive(&sock), host_keepalive(&sock));
        assert_eq!(
            (before, dialed, after),
            ((1, None), Some(true), (0, Some(false))),
            "SO_KEEPALIVE set before a connect must be held by the socket and reach the host \
             stream the Protocol Family dials; set once connected, it must reach that stream at \
             once"
        );
    }

    // @behavior RT-367
    // Windows sets keepalive timing through SIO_KEEPALIVE_VALS, which reads
    // nothing back, so only the hosts that read it witness the dialed values.
    #[cfg(not(windows))]
    #[test]
    fn a_socket_bounds_its_keepalive_probes_as_linux_and_dials_with_them() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let addr = listener.local_addr().unwrap();
        let sock = TcpSock::new(Arc::new(HostInet), &prepare_creds_within(&[], None))
            .expect("the reactor starts");
        let get = |opt| sock.getsockopt(opt).unwrap();
        let defaults = (
            get(SockOpt::KeepIdle),
            get(SockOpt::KeepIntvl),
            get(SockOpt::KeepCnt),
        );
        let refused = [
            sock.setsockopt(SockOpt::KeepIdle, 0),
            sock.setsockopt(SockOpt::KeepIdle, 32768),
            sock.setsockopt(SockOpt::KeepIntvl, 32768),
            sock.setsockopt(SockOpt::KeepCnt, 128),
        ];
        sock.setsockopt(SockOpt::KeepIdle, 30)
            .expect("an idle time Linux takes");
        sock.setsockopt(SockOpt::KeepIntvl, 10)
            .expect("an interval Linux takes");
        sock.setsockopt(SockOpt::KeepCnt, 3)
            .expect("a count Linux takes");
        sock.connect(vec![addr], addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        let host = sock.state().host.as_ref().map(|(stream, _)| {
            let host = SockRef::from(stream);
            (
                host.tcp_keepalive_time().unwrap(),
                host.tcp_keepalive_interval().unwrap(),
                host.tcp_keepalive_retries().unwrap(),
            )
        });
        assert_eq!(
            (defaults, refused, host),
            (
                (7200, 75, 9),
                [(); 4].map(|()| Err(FilesystemError::InvalidInput)),
                Some((Duration::from_secs(30), Duration::from_secs(10), 3))
            ),
            "a socket must read Linux's keepalive defaults, refuse with EINVAL an idle time or \
             interval outside 1 to 32767 and a count outside 1 to 127, and give the host stream it \
             dials the probes it was set"
        );
    }

    // @behavior RT-369
    #[test]
    fn a_socket_holds_its_hop_limit_for_the_host_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let addr = listener.local_addr().unwrap();
        let sock = TcpSock::new(Arc::new(HostInet), &prepare_creds_within(&[], None))
            .expect("the reactor starts");
        let host_ttl = |sock: &TcpSock| {
            sock.state()
                .host
                .as_ref()
                .map(|(stream, _)| SockRef::from(stream).ttl_v4().unwrap())
        };
        let default = sock.getsockopt(SockOpt::Ttl);
        let refused = [
            sock.setsockopt(SockOpt::Ttl, 0),
            sock.setsockopt(SockOpt::Ttl, 256),
        ];
        sock.setsockopt(SockOpt::Ttl, 7)
            .expect("a hop limit Linux takes");
        sock.connect(vec![addr], addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        let dialed = host_ttl(&sock);
        sock.setsockopt(SockOpt::Ttl, 9)
            .expect("a connected socket takes a hop limit");
        assert_eq!(
            (
                default,
                refused,
                dialed,
                sock.getsockopt(SockOpt::Ttl),
                host_ttl(&sock)
            ),
            (
                Ok(64),
                [(); 2].map(|()| Err(FilesystemError::InvalidInput)),
                Some(7),
                Ok(9),
                Some(9)
            ),
            "a TCP socket must read Linux's default hop limit rather than the host's, refuse zero \
             and one past 255 with EINVAL, and give the host stream it dials the limit set, at \
             once once connected"
        );
    }

    // @behavior RT-368
    #[test]
    fn a_socket_doubles_its_buffer_sizes_and_holds_its_queues_within_them() {
        let (sock, file, mut peer) = connected_socket();
        let get = |opt| sock.getsockopt(opt).unwrap() as usize;
        let defaults = (get(SockOpt::RcvBuf), get(SockOpt::SndBuf));
        sock.setsockopt(SockOpt::RcvBuf, 10_000_000).unwrap();
        let capped = get(SockOpt::RcvBuf);
        sock.setsockopt(SockOpt::SndBuf, 1000).unwrap();
        sock.setsockopt(SockOpt::RcvBuf, 1000).unwrap();
        let set = (get(SockOpt::RcvBuf), get(SockOpt::SndBuf), sock.send_room());
        peer.write_all(&[7; 10_000]).expect("the peer writes");
        wait_for(|table| file.poll(table), PollMask::IN);
        let held = sock.state().receive_queue.len();
        assert_eq!(
            (defaults, capped, set, held <= 2304),
            (
                (MAX_SOCKET_BUFFER, MAX_SOCKET_BUFFER),
                MAX_SOCKET_BUFFER,
                (2304, 4608, 4608),
                true
            ),
            "SO_RCVBUF and SO_SNDBUF must double a size as Linux does, within the socket's \
             reserved buffer and above Linux's least, and the queues must hold no more than they \
             say; the receive queue held {held}"
        );
    }

    // @behavior RT-342
    #[test]
    fn a_socket_connects_without_blocking_and_its_poll_wakes() {
        let (sock, _file, _peer) = connected_socket();
        assert_eq!(
            sock.finish_connect(),
            Ok(()),
            "TcpSock::finish_connect once the socket file's poll found it writable must succeed"
        );
    }

    // @behavior RT-356
    #[test]
    fn a_connected_socket_asked_to_connect_again_answers_eisconn() {
        let (sock, _file, _peer) = connected_socket();
        sock.finish_connect().expect("the socket connected");
        let addr = sock.getpeername().expect("a connected socket has a peer");
        let (was_reset, _reset_file) = reset_socket();
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
        let sock = dial(refused_at);
        let file = sock_alloc_file(sock.clone(), &Arc::default());
        wait_for(|table| file.poll(table), PollMask::ERR);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let open_at = listener.local_addr().unwrap();
        let connect = || sock.connect(vec![open_at], open_at, &OpenConnections::new(None));
        let first = connect();
        let second = connect();
        listener
            .accept()
            .expect("the listener accepts the fresh connect");
        wait_for(|table| file.poll(table), PollMask::OUT);
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
        let (_sock, mut file, mut peer) = connected_socket();
        let mut buf = [0; 8];
        let before = file.read(&mut buf);
        peer.write_all(b"hello").unwrap();
        drop(peer);
        let mut got = Vec::new();
        loop {
            wait_for(|table| file.poll(table), PollMask::IN);
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(err) => panic!("a read the poll found ready answered {err:?}"),
            }
        }
        assert_eq!(
            (before, got),
            (Err(FilesystemError::WouldBlock), b"hello".to_vec()),
            "File::read of a socket file must answer EAGAIN before anything arrived, then the \
             peer's bytes, then zero once it closed"
        );
    }

    // @behavior RT-351
    #[test]
    fn a_reset_reads_as_an_error_its_poll_reports() {
        let (_sock, mut file, peer) = connected_socket();
        file.write(b"unread").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        // A close with bytes unread sends a reset, as Linux's `tcp_close` does.
        drop(peer);
        wait_for(|table| file.poll(table), PollMask::ERR);
        assert_eq!(
            file.read(&mut [0; 8]),
            Err(FilesystemError::ConnectionReset),
            "a socket file's poll after the peer reset must find ERR, as tcp_poll reads sk_err, \
             and its read must answer ECONNRESET"
        );
    }

    /// A socket file whose peer reset it, as a close with bytes unread does.
    fn reset_socket() -> (Arc<TcpSock>, File) {
        let (sock, mut file, peer) = connected_socket();
        file.write(b"unread").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        drop(peer);
        wait_for(|table| file.poll(table), PollMask::ERR);
        (sock, file)
    }

    // @behavior RT-357
    #[test]
    fn a_reset_socket_answers_its_error_once_then_as_closed() {
        let (_sock, mut file) = reset_socket();
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

    // @behavior RT-376
    #[test]
    fn a_socket_its_peer_ended_reads_the_end_before_a_later_failure() {
        let (_sock, mut file, peer) = connected_socket();
        file.write(b"unread").unwrap();
        peer.shutdown(Shutdown::Write).unwrap();
        wait_for(|table| file.poll(table), PollMask::IN);
        std::thread::sleep(Duration::from_millis(20));
        drop(peer);
        std::thread::sleep(Duration::from_millis(20));
        let _ = file.write(b"after the reset");
        wait_for(|table| file.poll(table), PollMask::ERR);
        let mut buf = [0; 8];
        let (read, write) = (file.read(&mut buf), file.write(b"x"));
        assert!(
            read == Ok(0) && write.is_err(),
            "File::read of a socket whose peer sent its FIN, then reset it, must answer zero \
             ahead of the failure, as SOCK_DONE stops tcp_recvmsg before sk_err, and \
             File::write the failure; got {read:?} and {write:?}"
        );
    }

    // @behavior RT-377
    #[test]
    fn a_socket_shut_down_while_it_connects_disconnects() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let sock = dial(listener.local_addr().unwrap());
        // Hold it connecting whatever the reactor settled meanwhile, so the
        // shutdown meets a connect under way.
        sock.state().phase = Phase::Connecting;
        let shut = sock.shutdown(Shutdown::Both);
        let (finished, marked) = {
            let finished = sock.finish_connect();
            let state = sock.state();
            (finished, (state.inet.rcv_shut, state.inet.send_shut))
        };
        assert_eq!(
            (shut, finished, marked),
            (Ok(()), Err(FilesystemError::NotConnected), (false, false)),
            "a shutdown of a socket still connecting must answer success, leave it unconnected \
             so finishing the connect answers ENOTCONN, and mark neither side, as inet_shutdown \
             disconnects a socket in TCP_SYN_SENT"
        );
    }

    /// Write to `file` until the socket and the host take no more, answering
    /// how many bytes it took.
    fn fill(file: &mut File) -> usize {
        let mut sent = 0;
        loop {
            match file.write(&[7; 16 * 1024]) {
                Ok(n) => sent += n,
                Err(FilesystemError::WouldBlock) => return sent,
                Err(err) => panic!("a connected socket takes bytes until full: {err:?}"),
            }
        }
    }

    // @behavior RT-378
    #[test]
    fn a_released_socket_sends_what_it_queued_then_its_fin() {
        let (sock, mut file, mut peer) = connected_socket();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let sent = fill(&mut file);
        // Nothing but the socket itself holds it once its file is gone.
        drop((sock, file));
        let mut got = Vec::new();
        let read = peer.read_to_end(&mut got).map(|_| got.len());
        assert_eq!(
            read.ok(),
            Some(sent),
            "a socket file dropped with bytes still queued must send every one of the {sent} \
             bytes it took, then its FIN, as __tcp_close sends what is queued"
        );
    }

    // @behavior RT-379
    #[test]
    fn a_released_socket_the_peer_sends_to_resets_and_lets_go() {
        let (sock, _file, _peer) = connected_socket();
        // Released with a byte still queued, as an orphan holds itself, and
        // the peer's bytes already in: the host would answer them a reset.
        {
            let mut state = sock.state();
            state.orphan = Some(sock.clone());
            state.write_queue.push_back(7);
            state.receive_queue.extend(b"after the close");
        }
        sock.pump_and_wake(sock.state());
        let state = sock.state();
        assert_eq!(
            (state.orphan.is_some(), state.host.is_some()),
            (false, false),
            "a released socket its peer sends to must reset the connection and let go of \
             itself, bytes still queued or not, as Linux resets data arriving after __tcp_close"
        );
    }

    // @behavior RT-379
    #[test]
    fn a_socket_released_with_bytes_unread_resets_its_connection() {
        let (_sock, file, mut peer) = connected_socket();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        peer.write_all(b"unread").unwrap();
        wait_for(|table| file.poll(table), PollMask::IN);
        drop(file);
        let read = peer.read(&mut [0; 8]).map_err(|err| err.kind());
        assert_eq!(
            read,
            Err(io::ErrorKind::ConnectionReset),
            "a socket file dropped with bytes it never read must reset its connection, as \
             __tcp_close sends a reset when unread data is thrown away"
        );
    }

    // @behavior RT-358
    #[test]
    fn a_reset_or_refused_socket_polls_hung_up() {
        let closed = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let addr = closed.local_addr().unwrap();
        drop(closed);
        let refused = sock_alloc_file(dial(addr), &Arc::default());
        wait_for(|table| refused.poll(table), PollMask::ERR);
        let every = PollMask::IN | PollMask::OUT | PollMask::ERR | PollMask::HUP;
        assert_eq!(
            (reset_socket().1.poll(None), refused.poll(None)),
            (every, every),
            "a socket file whose peer reset it, and one whose connect was refused, must each \
             poll IN, OUT, ERR, and HUP, as tcp_poll answers TCP_CLOSE"
        );
    }

    // @behavior RT-359
    #[test]
    fn a_shut_down_reading_side_reads_what_arrived() {
        let (sock, mut file, mut peer) = connected_socket();
        peer.write_all(b"queued").unwrap();
        wait_for(|table| file.poll(table), PollMask::IN);
        sock.shutdown(Shutdown::Read)
            .expect("a connected socket shuts down");
        let mut buf = [0; 16];
        let first = file.read(&mut buf).map(|n| buf[..n].to_vec());
        assert_eq!(
            (first, file.read(&mut buf)),
            (Ok(b"queued".to_vec()), Ok(0)),
            "a socket's read after shutting its reading side down must still read the bytes \
             that arrived, then zero"
        );
    }

    // @behavior RT-360
    #[test]
    fn a_zero_byte_read_of_a_tcp_socket_answers_zero() {
        let (_sock, mut file, _peer) = connected_socket();
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
        let (sock, mut file, mut peer) = connected_socket();
        assert_eq!(
            file.write(b"ping"),
            Ok(4),
            "a connected socket takes the bytes"
        );
        sock.shutdown(Shutdown::Write)
            .expect("a connected socket shuts down");
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut got = Vec::new();
        peer.read_to_end(&mut got)
            .expect("the peer reads until the end");
        assert_eq!(
            got,
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
        let sock = dial(addr);
        let file = sock_alloc_file(sock.clone(), &Arc::default());
        wait_for(|table| file.poll(table), PollMask::OUT);
        assert_eq!(
            sock.finish_connect(),
            Err(FilesystemError::ConnectionRefused),
            "TcpSock::finish_connect to a port no peer listens on must answer ECONNREFUSED"
        );
    }

    // @behavior RT-346
    #[test]
    fn dropping_the_socket_file_releases_the_host_socket() {
        let (_sock, file, mut peer) = connected_socket();
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
