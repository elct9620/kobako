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

use super::connections::OpenConnections;
use super::family::ProtocolFamily;
use super::namespace::Namespace;
use super::reactor::{Reactor, Ready, Registration};
use super::socket::{
    connect4, ensure_within, filter, filter_with, set_hop_limit, take_connect_outcome, to_error,
    BindHash, Destination, InetSock, MemoryCharge, ProtoOps, SockOpt, MAX_SOCKET_BUFFER,
};
use crate::network::{Protocol, Ruleset};
use crate::vfs::{Cred, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};
use crate::Errno;

/// The most one host read moves at once.
const MAX_HOST_READ: usize = 16 * 1024;

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
    /// Whether each write leaves without Nagle's delay, as `TCP_NODELAY`
    /// sets `tp->nonagle`; Linux's default waits.
    nodelay: bool,
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
    this: Weak<Self>,
}

impl TcpSock {
    /// A socket connected to nothing, which connects through `family`, as
    /// `inet_create` makes one; its buffers are reserved against `cred`'s
    /// memory, and past it the socket answers `ENOMEM`.
    pub(crate) fn new(
        family: Arc<dyn ProtocolFamily>,
        cred: &Arc<Cred>,
    ) -> Result<Arc<Self>, Errno> {
        let memory = MemoryCharge::new(cred, 2 * MAX_SOCKET_BUFFER)?;
        Ok(Arc::new_cyclic(|this| Self {
            state: Mutex::new(State {
                phase: Phase::Unconnected,
                host: None,
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
                nodelay: false,
                orphan: None,
            }),
            wait: Arc::default(),
            family,
            this: this.clone(),
        }))
    }

    /// Whether the socket may begin a connect, as `__inet_stream_connect`
    /// switches on its state before anything else: one connecting answers
    /// `EALREADY`, one connected or reset `EISCONN`, and one a connect failed
    /// answers that failure once and stands unconnected again.
    fn check_state(&self) -> Result<(), Errno> {
        let mut state = self.state();
        match state.phase {
            Phase::Connecting => Err(Errno::AlreadyInProgress),
            Phase::Connected | Phase::Reset => Err(Errno::AlreadyConnected),
            Phase::Failed => {
                let err = state.inet.error.take().unwrap_or(Errno::ConnectionAborted);
                Self::disconnect(&mut state);
                Err(err)
            }
            Phase::Unconnected => Ok(()),
        }
    }

    /// Dial `addr`, the guest having named `peer`, as `tcp_v4_connect` sends
    /// the SYN to one destination; the connect goes on without blocking, so
    /// this returns at once and a poll finds the socket writable once it
    /// settles. The socket counts against `open_connections` and the process
    /// until released, and a full count answers `EAGAIN`, never a Trap, so one
    /// Sandbox cannot end another's run; the port it held goes back then, as
    /// `tcp_v4_connect`'s failure puts it.
    pub(crate) fn dial(
        &self,
        addr: SocketAddr,
        peer: SocketAddr,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<(), Errno> {
        let mut state = self.state();
        match state.inet.open_place(open_connections) {
            Ok(Some(place)) => state.inet.place = Some(place),
            Ok(None) => {}
            Err(err) => {
                state.inet.put_port();
                return Err(err);
            }
        }
        self.start_connect(&mut state, addr, peer);
        Ok(())
    }

    /// Connect to `dest`, the destination an egress proxy already judged by
    /// its name, through what the socket layer judges after the hooks: the
    /// socket's state, a port from `ports`, and `ruleset`'s filter. The hooks
    /// already judged the name the guest asked for, so they see no real
    /// address, and the request the proxy dials for already counts against
    /// the caps, so the socket takes no place of its own.
    pub(crate) fn connect_upstream(
        &self,
        ports: &Arc<BindHash>,
        ruleset: &Ruleset,
        dest: &Destination,
    ) -> Result<(), Errno> {
        self.check_state()?;
        self.hold_port(ports)?;
        let real = filter_with(ruleset, &*self.family, dest, Protocol::Http).inspect_err(|_| {
            self.state().inet.put_port();
        })?;
        self.start_connect(&mut self.state(), real, real);
        Ok(())
    }

    /// Begin connecting to `addr` for the guest's `peer`: no failure pending,
    /// as `tcp_connect_init` clears `sk_err`, then the host socket's dial.
    fn start_connect(&self, state: &mut State, addr: SocketAddr, peer: SocketAddr) {
        state.inet.error = None;
        state.inet.peer = Some(peer);
        state.phase = Phase::Connecting;
        self.dial_host(state, addr);
    }

    /// The queue a task or a poll waits on for this socket, as `sk_sleep`
    /// answers a socket's wait queue.
    pub(crate) fn sk_sleep(&self) -> &Arc<WaitQueueHead> {
        &self.wait
    }

    /// Hold a free port from `ports` as the guest's own, unless one is held.
    pub(crate) fn hold_port(&self, ports: &Arc<BindHash>) -> Result<(), Errno> {
        self.state().inet.hold_port(ports)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Start connecting the host socket to `addr`, or fail the connect. The
    /// dial registers on the reactor running as it dials, so a reactor whose
    /// poll failed meanwhile is replaced, not waited on.
    fn dial_host(&self, state: &mut State, addr: SocketAddr) {
        let dialed = Reactor::current().and_then(|reactor| {
            let stream = self.family.connect(addr)?;
            // The guest's own setsockopt already took each option, so one the
            // host socket refuses as it dials is no part of the connect's
            // outcome, which is what the guest reads.
            let _ = Self::set_options(SockRef::from(&stream), state);
            let mut stream = mio::net::TcpStream::from_std(stream);
            let ready: Weak<dyn Ready> = self.this.clone();
            let registration = reactor.register(&mut stream, ready)?;
            Ok((stream, registration))
        });
        match dialed {
            Ok(host) => state.host = Some(host),
            Err(err) => Self::done_with_error(state, to_error(&err)),
        }
    }

    /// Give the host socket `socket` the options the guest set.
    fn set_options(socket: SockRef<'_>, state: &State) -> io::Result<()> {
        socket.set_tcp_keepalive(&state.keepalive.to_params())?;
        socket.set_keepalive(state.keepalive.on)?;
        socket.set_tcp_nodelay(state.nodelay)?;
        set_hop_limit(socket, state.inet.ttl)
    }

    /// End the socket on `err`, as `tcp_done_with_error` does after a failed
    /// connect or a reset: `TCP_CLOSE`, both sides shut down, the failure
    /// pending.
    fn done_with_error(state: &mut State, err: Errno) {
        Self::close_host(state);
        state.inet.put_port();
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
        state.receive_queue.clear();
        state.received_end = false;
        state.inet.rcv_shut = false;
        state.write_queue.clear();
        state.inet.send_shut = false;
        state.fin_sent = false;
        state.inet.error = None;
        state.inet.peer = None;
        state.inet.put_port();
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
            Self::done_with_error(state, err);
        }
    }

    /// Read what arrived and write what is queued, until the host socket
    /// would block; a failure either way is the connection's end.
    fn move_bytes(state: &mut State) -> Result<(), Errno> {
        let Some((stream, _)) = state.host.as_mut() else {
            return Ok(());
        };
        let mut chunk = [0; MAX_HOST_READ];
        let rcvbuf = state.inet.buffers.rcvbuf;
        while state.receive_queue.len() < rcvbuf && !state.received_end {
            let room = (rcvbuf - state.receive_queue.len()).min(MAX_HOST_READ);
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
    /// non-blocking `connect`: connected, or failed with what the host saw.
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
        Self::done_with_error(state, to_error(&failure));
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

    /// How the connect ended, as a non-blocking socket's repeated `connect`
    /// answers in `__inet_stream_connect`: still under way answers EAGAIN
    /// where Linux answers `EALREADY`; connected answers success; a failure
    /// pending answers once, as `sock_error` clears `sk_err`, and an
    /// unconnected socket with none answers `ENOTCONN`.
    pub fn finish_connect(&self) -> Result<(), Errno> {
        let mut state = self.state();
        match &state.phase {
            Phase::Connecting => Err(Errno::WouldBlock),
            Phase::Connected => Ok(()),
            Phase::Unconnected | Phase::Failed | Phase::Reset => {
                Err(state.inet.error.take().unwrap_or(Errno::NotConnected))
            }
        }
    }

    /// Whether a connect is under way, as a socket in `TCP_SYN_SENT` is.
    pub fn is_connecting(&self) -> bool {
        self.state().phase == Phase::Connecting
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
    fn set_options_with(&self, change: impl FnOnce(&mut State)) -> Result<(), Errno> {
        let mut state = self.state();
        let kept = (state.keepalive, state.nodelay, state.inet.ttl);
        change(&mut state);
        let Some((stream, _)) = &state.host else {
            return Ok(());
        };
        let set = Self::set_options(SockRef::from(stream), &state);
        set.map_err(|err| {
            (state.keepalive, state.nodelay, state.inet.ttl) = kept;
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
    /// The socket's state first, then `net`'s `connect4`, then within the
    /// connection caps a port, the ruleset, and the dial, as
    /// `inet_stream_connect` runs `connect4` only on a socket connected to
    /// nothing and `tcp_v4_connect` hashes a port before the SYN meets the
    /// filter. A refused destination puts the port back, as that failure
    /// does.
    fn connect(&self, net: &Namespace, addr: SocketAddr) -> Result<(), Errno> {
        self.check_state()?;
        let dest = connect4(net, addr)?;
        net.within_connection_budget(Errno::WouldBlock, || {
            self.hold_port(net.ports_of(Protocol::Tcp))?;
            let real = filter(net, &dest, Protocol::Tcp).inspect_err(|_| {
                self.state().inet.put_port();
            })?;
            self.dial(real, addr, net.open_connections())
        })
    }

    /// A socket connecting or connected holds its own end already, as
    /// `inet_hash_connect` binds one past `TCP_CLOSE`; one a failed connect or
    /// a reset closed is back in `TCP_CLOSE`.
    fn bind(&self, net: &Namespace, addr: SocketAddr) -> Result<(), Errno> {
        let mut state = self.state();
        let connected = matches!(state.phase, Phase::Connecting | Phase::Connected);
        state
            .inet
            .bind(addr, connected, net.ports_of(Protocol::Tcp))
    }

    /// What the receive buffer holds, up to `buf`'s length, as
    /// `tcp_recvmsg_locked` copies what is queued first. A drained buffer then
    /// answers the peer's end as zero, ahead of any failure as `SOCK_DONE`
    /// is, then a pending failure once; it answers EAGAIN until either comes,
    /// and ENOTCONN on a socket that never connected.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddr>), Errno> {
        let mut state = self.state();
        if !state.receive_queue.is_empty() {
            let take = state.receive_queue.len().min(buf.len());
            for (slot, byte) in buf.iter_mut().zip(state.receive_queue.drain(..take)) {
                *slot = byte;
            }
            self.pump_and_wake(state);
            return Ok((take, None));
        }
        if state.received_end {
            return Ok((0, None));
        }
        if let Some(err) = state.inet.error.take() {
            return Err(err);
        }
        match state.phase {
            _ if state.inet.rcv_shut => Ok((0, None)),
            Phase::Failed | Phase::Reset => Ok((0, None)),
            Phase::Unconnected => Err(Errno::NotConnected),
            Phase::Connecting | Phase::Connected => Err(Errno::WouldBlock),
        }
    }

    /// Queue what fits of `buf`, sent as the host socket takes it; a full
    /// buffer answers EAGAIN. A socket not connected answers as
    /// `sk_stream_wait_connect` does: EAGAIN while it connects, a pending
    /// failure once, then EPIPE. A connected one past its failure or its
    /// writing side's shutdown answers as `sk_stream_error`: the failure
    /// once, then EPIPE.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, Errno> {
        let mut state = self.state();
        match state.phase {
            Phase::Connecting => return Err(Errno::WouldBlock),
            Phase::Connected if !state.inet.send_shut && state.inet.error.is_none() => {}
            _ => return Err(state.inet.error.take().unwrap_or(Errno::BrokenPipe)),
        }
        let take = buf.len().min(
            state
                .inet
                .buffers
                .sndbuf
                .saturating_sub(state.write_queue.len()),
        );
        if take == 0 && !buf.is_empty() {
            return Err(Errno::WouldBlock);
        }
        state.write_queue.extend(&buf[..take]);
        self.pump_and_wake(state);
        Ok(take)
    }

    /// As `tcp_poll` answers: hung up in `TCP_CLOSE` or with both sides shut
    /// down, readable once the reading side is, and writable unless it
    /// connects, always so once its writing side is shut down and otherwise
    /// once the room left is half what is queued, as
    /// `__sk_stream_is_writeable` asks.
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.wait, table);
        let state = self.state();
        let closed = state.phase == Phase::Unconnected || state.phase.is_closed();
        let rcv_shut = state.received_end || state.inet.rcv_shut || state.phase.is_closed();
        let send_shut = state.inet.send_shut || state.phase.is_closed();
        let mut mask = PollMask::default();
        if closed || (rcv_shut && send_shut) {
            mask = mask | PollMask::HUP;
        }
        if !state.receive_queue.is_empty() || rcv_shut {
            mask = mask | PollMask::IN;
        }
        let queued = state.write_queue.len();
        let writable = state.inet.buffers.sndbuf.saturating_sub(queued) >= queued / 2;
        if state.phase != Phase::Connecting && (send_shut || writable) {
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
    /// back to unconnected with nothing marked and `ECONNRESET` pending, as
    /// `inet_shutdown` disconnects one in `TCP_SYN_SENT` and
    /// `tcp_disconnect` sets `sk_err`; it keeps its place until released.
    fn shutdown(&self, how: Shutdown) -> Result<(), Errno> {
        let mut state = self.state();
        if state.phase == Phase::Connecting {
            Self::close_host(&mut state);
            Self::disconnect(&mut state);
            state.inet.error = Some(Errno::ConnectionReset);
            drop(state);
            self.wait.wake_up();
            return Ok(());
        }
        let connected = state.phase == Phase::Connected;
        let shut = state.inet.shutdown(how, connected);
        self.pump_and_wake(state);
        shut
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
    fn getsockopt(&self, opt: SockOpt) -> Result<u32, Errno> {
        let state = self.state();
        match opt {
            SockOpt::KeepAlive => Ok(u32::from(state.keepalive.on)),
            SockOpt::KeepIdle => Ok(state.keepalive.idle),
            SockOpt::KeepIntvl => Ok(state.keepalive.interval),
            SockOpt::KeepCnt => Ok(state.keepalive.count),
            SockOpt::NoDelay => Ok(u32::from(state.nodelay)),
            _ => state.inet.getsockopt(opt),
        }
    }

    /// The probes and the hop limit reach a host socket already dialed at
    /// once; each probe bound is Linux's, as `tcp_sock_set_keepidle` and its
    /// kin refuse past it with EINVAL.
    fn setsockopt(&self, opt: SockOpt, value: u32) -> Result<(), Errno> {
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
            SockOpt::NoDelay => self.set_options_with(|state| state.nodelay = value != 0),
            _ => {
                let mut guard = self.state();
                let state = &mut *guard;
                let host = state.host.as_ref().map(|(stream, _)| SockRef::from(stream));
                state.inet.setsockopt(opt, value, host)
            }
        }
    }

    /// Let go of the socket once every view is gone, as `sock_close` runs on
    /// the last `fput` and `__tcp_close` closes it: bytes still queued leave
    /// before the FIN, the socket holding itself and its place until then,
    /// and bytes that arrived unread reset the connection instead.
    fn release(&self) {
        let mut state = self.state();
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
            .expect("an unbounded socket is made");
        sock.dial(addr, addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        sock
    }

    /// A namespace whose ruleset admits 127.0.0.1 on `ports`, its sockets
    /// counting against `open`.
    fn loopback(ports: &[u16], open: Arc<OpenConnections>) -> Namespace {
        use crate::net::HyperOutbound;
        use crate::network::{Builder, Endpoint, Network};
        let mut entry = Builder::default();
        for port in ports {
            entry.endpoint(
                Endpoint::new("127.0.0.1", *port)
                    .unwrap()
                    .with_protocol(Protocol::Tcp),
            );
        }
        let network = Network::new("loopback", Arc::new(entry)).unwrap();
        Namespace::new(
            [&network],
            Arc::new(HostInet),
            Arc::new(HostInet),
            Arc::new(HyperOutbound::new()),
            open,
            None,
        )
        .expect("the endpoints compile")
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
            (Err(Errno::WouldBlock), true),
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

    #[test]
    fn a_socket_asked_for_tcp_nodelay_gives_it_to_its_host_socket() {
        let (sock, _file, _peer) = connected_socket();
        sock.setsockopt(SockOpt::NoDelay, 1)
            .expect("TCP takes TCP_NODELAY");
        let nodelay = sock
            .state()
            .host
            .as_ref()
            .map(|(stream, _)| stream.nodelay().unwrap());
        assert_eq!(
            (sock.getsockopt(SockOpt::NoDelay), nodelay),
            (Ok(1), Some(true)),
            "a TCP socket asked for TCP_NODELAY must read it back and pass it to the host \
             socket it dialed, so each write leaves without Nagle's delay"
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
            .expect("an unbounded socket is made");
        sock.setsockopt(SockOpt::KeepAlive, 1)
            .expect("an unconnected socket takes an option");
        let keepalive = |sock: &TcpSock| sock.getsockopt(SockOpt::KeepAlive).unwrap();
        let before = (keepalive(&sock), host_keepalive(&sock));
        sock.dial(addr, addr, &OpenConnections::new(None))
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
            .expect("an unbounded socket is made");
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
        sock.dial(addr, addr, &OpenConnections::new(None))
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
                [(); 4].map(|()| Err(Errno::InvalidInput)),
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
            .expect("an unbounded socket is made");
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
        sock.dial(addr, addr, &OpenConnections::new(None))
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
                [(); 2].map(|()| Err(Errno::InvalidInput)),
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

    // @behavior RT-385
    #[test]
    fn a_socket_binds_once_and_not_after_its_connect() {
        let any: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let net = loopback(&[], OpenConnections::new(None));
        let cred = prepare_creds_within(&[], None);
        let fresh = TcpSock::new(Arc::new(HostInet), &cred).expect("an unbounded socket is made");
        let (first, second) = (fresh.bind(&net, any), fresh.bind(&net, any));
        let ipv6 = TcpSock::new(Arc::new(HostInet), &cred)
            .expect("an unbounded socket is made")
            .bind(&net, "[::]:0".parse().unwrap());
        let (connected, _file, _peer) = connected_socket();
        let udp_peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp = crate::net::UdpSock::new(&cred).expect("an unbounded socket is made");
        let family: Arc<dyn ProtocolFamily> = Arc::new(HostInet);
        let at = udp_peer.local_addr().unwrap();
        udp.dial(&family, at, at, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        assert_eq!(
            (
                first,
                second,
                ipv6,
                connected.bind(&net, any),
                udp.bind(&net, any)
            ),
            (
                Ok(()),
                Err(Errno::InvalidInput),
                Err(Errno::AddressFamilyNotSupported),
                Err(Errno::InvalidInput),
                Err(Errno::InvalidInput)
            ),
            "a socket's bind must take its own end once, answer EINVAL to a second bind and to a \
             bind after a TCP or UDP connect bound it, and EAFNOSUPPORT to IPv6, as Linux's \
             inet_bind"
        );
    }

    // @behavior RT-386
    #[test]
    fn a_failed_connect_gives_back_a_port_its_bind_did_not_name() {
        // A privileged port no test can bind, so every connect to it is
        // refused and no other socket is handed it meanwhile.
        let closed = SocketAddr::from(([127, 0, 0, 1], 1));
        let cred = prepare_creds_within(&[], None);
        let net = loopback(&[], OpenConnections::new(None));
        let failed_after = |own: &str| {
            let sock =
                TcpSock::new(Arc::new(HostInet), &cred).expect("an unbounded socket is made");
            sock.bind(&net, own.parse().unwrap())
                .expect("a fresh socket binds");
            sock.dial(closed, closed, &OpenConnections::new(None))
                .expect("an uncapped socket connects");
            let file = sock_alloc_file(sock.clone(), &Arc::default());
            wait_for(|table| file.poll(table), PollMask::ERR);
            sock.bind(&net, "0.0.0.0:0".parse().unwrap())
        };
        assert_eq!(
            (failed_after("0.0.0.0:0"), failed_after("127.0.0.1:40000")),
            (Ok(()), Err(Errno::InvalidInput)),
            "a TCP socket bound to port 0 must bind again once its connect to a closed port fails, \
             and one bound to a named port must still answer EINVAL, as tcp_set_state puts the \
             port on TCP_CLOSE unless SOCK_BINDPORT_LOCK keeps it"
        );
    }

    // @behavior RT-390
    #[test]
    fn a_port_one_socket_holds_is_no_other_sockets() {
        let net = loopback(&[], OpenConnections::new(None));
        let cred = prepare_creds_within(&[], None);
        let socket =
            || TcpSock::new(Arc::new(HostInet), &cred).expect("an unbounded socket is made");
        let (named, free, again) = (socket(), socket(), socket());
        let answers = (
            named.bind(&net, "0.0.0.0:32768".parse().unwrap()),
            free.bind(&net, "0.0.0.0:0".parse().unwrap()),
            again.bind(&net, "0.0.0.0:32768".parse().unwrap()),
        );
        let free_port = free.getsockname().port();
        drop(named);
        assert_eq!(
            (
                answers,
                free_port,
                again.bind(&net, "0.0.0.0:32768".parse().unwrap())
            ),
            ((Ok(()), Ok(()), Err(Errno::AddressInUse)), 32769, Ok(())),
            "a TCP socket's bind must pass over a port another socket holds for port 0, answer \
             EADDRINUSE to it by name, and take it once the holder is gone, as \
             inet_csk_get_port finds a bucket taken"
        );
    }

    // @behavior RT-389
    #[test]
    fn a_connect_refused_for_a_full_count_or_by_the_ruleset_gives_back_its_port() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let at = listener.local_addr().unwrap();
        let cred = prepare_creds_within(&[], None);
        let refused_after = |net: &Namespace, to: SocketAddr| {
            let sock =
                TcpSock::new(Arc::new(HostInet), &cred).expect("an unbounded socket is made");
            let connected = ProtoOps::connect(&*sock, net, to);
            (connected, sock.bind(net, "0.0.0.0:0".parse().unwrap()))
        };
        let full = loopback(&[at.port()], OpenConnections::new(Some(0)));
        let filtered = loopback(&[at.port()], OpenConnections::new(None));
        assert_eq!(
            (
                refused_after(&full, at),
                refused_after(&filtered, SocketAddr::from(([127, 0, 0, 1], 1)))
            ),
            (
                (Err(Errno::WouldBlock), Ok(())),
                (Err(Errno::PermissionDenied), Ok(()))
            ),
            "a TCP socket's connect under a full open connection count, and one to a destination \
             the ruleset refuses, must answer EAGAIN and EACCES and give back the port it held, \
             so a bind to port 0 then succeeds, as tcp_v4_connect's failure puts the port it \
             hashed"
        );
    }

    // @behavior RT-384
    #[test]
    fn a_socket_polls_writable_once_its_room_is_half_what_it_queued() {
        let (sock, file, _peer) = connected_socket();
        sock.setsockopt(SockOpt::SndBuf, 1000).unwrap();
        let polled_with = |queued: usize| {
            let mut state = sock.state();
            if let Some((stream, registration)) = state.host.as_mut() {
                registration.deregister(stream);
            }
            state.write_queue = std::iter::repeat_n(0, queued).collect();
            drop(state);
            file.poll(None).contains(PollMask::OUT)
        };
        assert_eq!(
            (polled_with(3072), polled_with(3073)),
            (true, false),
            "a TCP socket whose send buffer reads 4608 must poll writable with 3072 bytes queued, \
             its room then half of them, and not with 3073, as __sk_stream_is_writeable asks"
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
        // The ruleset admits the peer only, under a cap already spent.
        let net = loopback(&[addr.port()], OpenConnections::new(Some(0)));
        let refused = SocketAddr::from(([127, 0, 0, 1], 1));
        let again = |sock: &TcpSock, to| ProtoOps::connect(sock, &net, to);
        assert_eq!(
            [
                again(&sock, addr),
                again(&was_reset, addr),
                again(&sock, refused)
            ],
            [
                Err(Errno::AlreadyConnected),
                Err(Errno::AlreadyConnected),
                Err(Errno::AlreadyConnected)
            ],
            "a TCP socket's connect on a connected socket, and on one connected then reset, must \
             answer EISCONN, as __inet_stream_connect does, before the ruleset judges the \
             destination or the connection caps are asked"
        );
    }

    // @behavior RT-364
    #[test]
    fn a_socket_whose_connect_failed_answers_the_failure_then_connects_afresh() {
        let closed = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let refused_at = closed.local_addr().unwrap();
        drop(closed);
        // One place, which the failed socket still holds, so connecting
        // afresh must reuse it rather than ask the full cap for another.
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let open_at = listener.local_addr().unwrap();
        let net = loopback(
            &[refused_at.port(), open_at.port()],
            OpenConnections::new(Some(1)),
        );
        let sock = TcpSock::new(Arc::new(HostInet), &prepare_creds_within(&[], None))
            .expect("an unbounded socket is made");
        ProtoOps::connect(&*sock, &net, refused_at).expect("a socket under a cap of one connects");
        let file = sock_alloc_file(sock.clone(), &Arc::default());
        wait_for(|table| file.poll(table), PollMask::ERR);
        let connect = || ProtoOps::connect(&*sock, &net, open_at);
        let message = "a TCP socket's connect after its connect was refused must answer \
                       ECONNREFUSED and dial nothing, then connect afresh under a cap of one, on \
                       the place it holds, with none of the failure left, as Linux disconnects a \
                       failed socket";
        let tries = (connect(), connect());
        assert_eq!(tries, (Err(Errno::ConnectionRefused), Ok(())), "{message}");
        listener
            .accept()
            .expect("the listener accepts the fresh connect");
        wait_for(|table| file.poll(table), PollMask::OUT);
        assert_eq!(
            (
                sock.finish_connect(),
                file.poll(None).contains(PollMask::ERR)
            ),
            (Ok(()), false),
            "{message}"
        );
    }

    // @behavior RT-388
    #[test]
    fn a_socket_whose_connect_failed_takes_a_fresh_virtual_port() {
        let closed = SocketAddr::from(([127, 0, 0, 1], 1));
        let sock = TcpSock::new(Arc::new(HostInet), &prepare_creds_within(&[], None))
            .expect("an unbounded socket is made");
        let file = sock_alloc_file(sock.clone(), &Arc::default());
        let ports = BindHash::new();
        // The port is held before the dial, as a connect holds it.
        sock.hold_port(&ports).expect("a port is free");
        let before = sock.getsockname().port();
        sock.dial(closed, closed, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        wait_for(|table| file.poll(table), PollMask::ERR);
        let failed = sock.finish_connect();
        let kept = sock.getsockname().port();
        sock.hold_port(&ports).expect("a port is free");
        assert_eq!(
            (failed, before, kept, sock.getsockname().port()),
            (Err(Errno::ConnectionRefused), 32768, 32768, 32769),
            "a TCP socket whose connect failed must read its old virtual port until it connects \
             again, then take a fresh one, as a hash connect picks a port once __inet_put_port \
             cleared inet_num"
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
            (Err(Errno::WouldBlock), b"hello".to_vec()),
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
            Err(Errno::ConnectionReset),
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
            (Err(Errno::ConnectionReset), Ok(0), Err(Errno::BrokenPipe)),
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
        let errored = sock.poll(None).contains(PollMask::ERR);
        let (first, second) = (sock.finish_connect(), sock.finish_connect());
        let (marked, placed) = {
            let state = sock.state();
            (
                (state.inet.rcv_shut, state.inet.send_shut),
                state.inet.place.is_some(),
            )
        };
        assert_eq!(
            (shut, errored, first, second, marked, placed),
            (
                Ok(()),
                true,
                Err(Errno::ConnectionReset),
                Err(Errno::NotConnected),
                (false, false),
                true
            ),
            "a shutdown of a socket still connecting must answer success and leave it \
             unconnected with ECONNRESET pending, its poll showing ERR, so finishing the connect \
             answers ECONNRESET once and then ENOTCONN, mark neither side, and keep its place \
             until released, as inet_shutdown disconnects a socket in TCP_SYN_SENT and \
             tcp_disconnect sets sk_err"
        );
    }

    /// Write to `file` until the socket and the host take no more, answering
    /// how many bytes it took.
    fn fill(file: &mut File) -> usize {
        let mut sent = 0;
        loop {
            match file.write(&[7; 16 * 1024]) {
                Ok(n) => sent += n,
                Err(Errno::WouldBlock) => return sent,
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
            Err(Errno::ConnectionRefused),
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
