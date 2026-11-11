//! A UDP socket the guest holds, connected to one peer, as Linux's `udp_sock`
//! behind a socket file. The host socket is non-blocking and the reactor
//! watches it: each change reads what arrived into the receive queue, then
//! wakes the socket's wait queue. A send goes to the host socket at once, as
//! `udp_sendmsg` sends, and a read takes one datagram, as `recv` on a
//! datagram socket does.

use std::collections::VecDeque;
use std::io;
use std::net::{Shutdown, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super::connections::{open, OpenConnections};
use super::family::ProtocolFamily;
use super::reactor::{Reactor, Ready, Registration};
use socket2::SockRef;

use super::socket::{
    set_hop_limit, to_error, to_ttl, InetSock, MemoryCharge, ProtoOps, SockOpt, MAX_SOCKET_BUFFER,
};
use crate::vfs::{Cred, FilesystemError, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// The most one IPv4 datagram carries.
pub const MAX_DATAGRAM: usize = 65_507;

/// What the receive queue spends on a datagram beside its bytes, as Linux's
/// `skb->truesize` counts the `sk_buff` itself, so even empty datagrams fill
/// `rcvbuf`.
const DATAGRAM_OVERHEAD: usize = std::mem::size_of::<(Vec<u8>, SocketAddr)>();

/// The most datagrams one pump takes from the host socket, as
/// `NAPI_POLL_WEIGHT` bounds one poll, so a flooded socket holds neither the
/// reactor nor a guest call.
const MAX_PUMPED_DATAGRAMS: usize = 64;

/// What a queued datagram of `len` bytes spends of `rcvbuf`.
fn truesize_of(len: usize) -> usize {
    len + DATAGRAM_OVERHEAD
}

struct State {
    /// The host socket, and its place in the reactor, until released.
    host: Option<(mio::net::UdpSocket, Registration)>,
    /// The datagrams that arrived and the guest has not read, each with the
    /// peer it came from as the guest named it, as `sk_receive_queue` keeps
    /// each skb's source, and the bytes they hold.
    receive_queue: VecDeque<(Vec<u8>, SocketAddr)>,
    rmem_alloc: usize,
    /// Whether the host socket took the last send, until the reactor next
    /// sees it change, as `sock_writeable` reads its room.
    writable: bool,
    /// What every inet socket holds. A send goes
    /// to the host at once, so `SO_SNDBUF` only reads back; once the reading
    /// side is shut, an empty queue still answers EAGAIN, as `__skb_recv_udp`
    /// does, and once the writing side is, a send answers `EPIPE`.
    inet: InetSock,
}

/// One UDP socket, shared by the socket file and the reactor.
pub struct UdpSock {
    state: Mutex<State>,
    wait: Arc<WaitQueueHead>,
    this: Weak<Self>,
}

impl UdpSock {
    /// A socket connected to nothing, as `inet_create` makes one; its receive
    /// queue is reserved against `cred`'s memory, and past it the socket
    /// answers `ENOMEM`.
    pub(crate) fn new(cred: &Arc<Cred>) -> Result<Arc<Self>, FilesystemError> {
        let memory = MemoryCharge::new(cred, MAX_SOCKET_BUFFER)?;
        Ok(Arc::new_cyclic(|this| Self {
            state: Mutex::new(State {
                host: None,
                receive_queue: VecDeque::new(),
                rmem_alloc: 0,
                writable: true,
                inet: InetSock::new(memory),
            }),
            wait: Arc::default(),
            this: this.clone(),
        }))
    }

    /// Connect to `addr` through `family`, the guest having named `peer`,
    /// the host socket watched by the reactor; it counts against
    /// `open_connections` and the process until released, and a full count
    /// answers `EAGAIN`. Connecting again moves the peer and keeps the place,
    /// and within one family the host socket and its port, as
    /// `__ip4_datagram_connect` repoints a socket.
    pub(crate) fn connect(
        &self,
        family: &Arc<dyn ProtocolFamily>,
        addr: SocketAddr,
        peer: SocketAddr,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<(), FilesystemError> {
        {
            let mut state = self.state();
            let same_family = state.host.as_ref().is_some_and(|(socket, _)| {
                socket
                    .local_addr()
                    .is_ok_and(|local| local.is_ipv4() == addr.is_ipv4())
            });
            if same_family {
                // What the old peer sent reads as its own, so it is taken in
                // before the host socket repoints.
                Self::pump(&mut state);
                if let Some((socket, _)) = state.host.as_ref() {
                    socket.connect(addr).map_err(|err| to_error(&err))?;
                }
                state.inet.peer = Some(peer);
                state.writable = true;
                return Ok(());
            }
        }
        let place = match self.state().inet.place.is_some() {
            true => None,
            false => Some(open(open_connections).ok_or(FilesystemError::WouldBlock)?),
        };
        let reactor = Reactor::current().map_err(|err| to_error(&err))?;
        let host = family
            .connect_datagram(addr)
            .and_then(|socket| {
                set_hop_limit(SockRef::from(&socket), self.state().inet.ttl).map(|()| socket)
            })
            .map_err(|err| to_error(&err))?;
        let mut socket = mio::net::UdpSocket::from_std(host);
        let ready: Weak<dyn Ready> = self.this.clone();
        let registration = reactor
            .register(&mut socket, ready)
            .map_err(|err| to_error(&err))?;
        let mut state = self.state();
        // What the old peer sent reads as its own, so it is taken in before
        // the host socket it waits in goes.
        Self::pump(&mut state);
        if let Some((mut earlier, registration)) = state.host.take() {
            registration.deregister(&mut earlier);
        }
        state.host = Some((socket, registration));
        state.inet.peer = Some(peer);
        state.writable = true;
        if let Some(place) = place {
            state.inet.place = Some(place);
        }
        Ok(())
    }

    /// One datagram, cut to `buf`, and the peer it came from as the guest
    /// named it, as `udp_recvmsg` fills `msg_name` with each one's source. A
    /// pending failure is answered first, as `recvmsg` answers it. A read
    /// takes in what a bounded pump left waiting in the host socket.
    pub fn recvfrom(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr), FilesystemError> {
        let mut state = self.state();
        if let Some(err) = state.inet.error.take() {
            return Err(err);
        }
        let Some((datagram, from)) = state.receive_queue.pop_front() else {
            return Err(FilesystemError::WouldBlock);
        };
        state.rmem_alloc -= truesize_of(datagram.len());
        Self::pump(&mut state);
        let n = buf.len().min(datagram.len());
        buf[..n].copy_from_slice(&datagram[..n]);
        Ok((n, from))
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

    /// Read what arrived, without blocking and at most `MAX_PUMPED_DATAGRAMS`
    /// at once, each labelled with the peer the guest named; one an earlier
    /// peer sent as the socket repointed is dropped, as a datagram may be
    /// lost. A full receive queue drops what comes, as a full `sk_rcvbuf`
    /// does. A failure the host reports is kept pending and the queue reads
    /// on behind it, as datagrams queue behind `sk_err`.
    fn pump(state: &mut State) {
        let Some((socket, _)) = state.host.as_ref() else {
            return;
        };
        let host_peer = socket.peer_addr().ok();
        let peer = state.inet.peer;
        let mut datagram = vec![0; MAX_DATAGRAM];
        let mut taken = 0;
        while taken < MAX_PUMPED_DATAGRAMS {
            match socket.recv_from(&mut datagram) {
                Ok((n, from)) => {
                    taken += 1;
                    let Some(peer) = peer.filter(|_| host_peer == Some(from)) else {
                        continue;
                    };
                    if state.rmem_alloc + truesize_of(n) <= state.inet.buffers.rcvbuf {
                        state
                            .receive_queue
                            .push_back((datagram[..n].to_vec(), peer));
                        state.rmem_alloc += truesize_of(n);
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => {
                    taken += 1;
                    state.inet.error = Some(to_udp_error(&err));
                }
            }
        }
    }

    fn pump_and_wake(&self, mut state: MutexGuard<'_, State>) {
        Self::pump(&mut state);
        drop(state);
        self.wait.wake_up();
    }
}

impl Ready for UdpSock {
    /// What arrived reads into the queue, and the host socket may take a send
    /// again.
    fn on_event(&self) {
        let mut state = self.state();
        state.writable = true;
        self.pump_and_wake(state);
    }
}

/// UDP's answers for its socket file, as `inet_dgram_ops` hands each call to
/// `udp_prot`.
impl ProtoOps for UdpSock {
    /// A socket connected to a peer holds its own end already, as
    /// `udp_pre_connect` autobinds one.
    fn bind(&self, addr: SocketAddr) -> Result<(), FilesystemError> {
        let mut state = self.state();
        let connected = state.inet.peer.is_some();
        state.inet.bind(addr, connected)
    }

    /// One datagram, cut to `buf`, as a datagram socket's read takes one. A
    /// pending failure is answered first, once, and then cleared, as
    /// `__skb_recv_udp` asks `sock_error` before it dequeues, so the socket
    /// reads on. An empty queue answers `WouldBlock` even once its reading
    /// side is shut, as a non-blocking `__skb_recv_udp` answers EAGAIN and
    /// only a blocking wait ends on `RCV_SHUTDOWN`.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        self.recvfrom(buf).map(|(n, _)| n)
    }

    /// One datagram, sent whole and at once, as `udp_sendmsg` sends: one past
    /// what UDP carries answers EMSGSIZE, one with no peer to go to
    /// EDESTADDRREQ, and a pending failure is answered once first. Once its
    /// writing side is shut a send answers EPIPE, as `sock_alloc_send_pskb`
    /// refuses on `SEND_SHUTDOWN`. A host socket with no room answers
    /// `WouldBlock` and sends nothing, so no datagram is taken that a release
    /// could lose.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        if buf.len() > MAX_DATAGRAM {
            return Err(FilesystemError::MessageTooLong);
        }
        let mut state = self.state();
        if let Some(err) = state.inet.error.take() {
            return Err(err);
        }
        if state.inet.peer.is_none() {
            return Err(FilesystemError::DestinationAddressRequired);
        }
        if state.inet.send_shut {
            return Err(FilesystemError::BrokenPipe);
        }
        let Some((socket, _)) = state.host.as_ref() else {
            return Err(FilesystemError::WouldBlock);
        };
        let sent = loop {
            match socket.send(buf) {
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                sent => break sent,
            }
        };
        match sent {
            Ok(_) => Ok(buf.len()),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                state.writable = false;
                Err(FilesystemError::WouldBlock)
            }
            Err(err) => Err(to_udp_error(&err)),
        }
    }

    /// As `datagram_poll` answers: readable once a datagram waits or its
    /// reading side is shut, writable while the host socket takes a send,
    /// hung up once both sides are shut, and failed while a failure is
    /// pending.
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.wait, table);
        let state = self.state();
        let mut mask = PollMask::default();
        if !state.receive_queue.is_empty() || state.inet.rcv_shut {
            mask = mask | PollMask::IN;
        }
        if (state.host.is_some() && state.writable) || state.inet.peer.is_none() {
            mask = mask | PollMask::OUT;
        }
        if state.inet.rcv_shut && state.inet.send_shut {
            mask = mask | PollMask::HUP;
        }
        if state.inet.error.is_some() {
            mask = mask | PollMask::ERR;
        }
        mask
    }

    /// Mark the sides `how` names shut, as `inet_shutdown` does; a socket
    /// connected to no peer marks them and answers `ENOTCONN`.
    fn shutdown(&self, how: Shutdown) -> Result<(), FilesystemError> {
        let mut state = self.state();
        state.inet.mark_shutdown(how);
        let connected = state.inet.peer.is_some();
        drop(state);
        self.wait.wake_up();
        match connected {
            true => Ok(()),
            false => Err(FilesystemError::NotConnected),
        }
    }

    fn getpeername(&self) -> Option<SocketAddr> {
        self.state().inet.peer
    }

    fn getsockname(&self) -> SocketAddr {
        self.state().inet.getsockname()
    }

    /// The buffers and the hop limit; UDP has no TCP option.
    fn getsockopt(&self, opt: SockOpt) -> Result<u32, FilesystemError> {
        self.state().inet.getsockopt(opt)
    }

    /// A buffer past it drops a datagram, as a full `sk_rcvbuf` does; a send
    /// goes to the host at once, so `SO_SNDBUF` only reads back. A hop limit
    /// reaches a host socket already there at once.
    fn setsockopt(&self, opt: SockOpt, value: u32) -> Result<(), FilesystemError> {
        let mut state = self.state();
        if opt != SockOpt::Ttl {
            return state.inet.set_buffer(opt, value);
        }
        let ttl = to_ttl(value)?;
        if let Some((socket, _)) = &state.host {
            set_hop_limit(SockRef::from(socket), ttl).map_err(|err| to_error(&err))?;
        }
        state.inet.ttl = ttl;
        Ok(())
    }

    /// Let the host socket go, deregistered from the reactor.
    fn release(&self) {
        let mut state = self.state();
        state.inet.release();
        state.receive_queue.clear();
        state.rmem_alloc = 0;
        if let Some((mut socket, registration)) = state.host.take() {
            registration.deregister(&mut socket);
        }
        drop(state);
        self.wait.wake_up();
    }
}

/// A host failure as Linux's UDP answers it. Windows reports an ICMP port
/// unreachable as `WSAECONNRESET`, where Linux's `__udp4_lib_err` sets
/// `ECONNREFUSED`, and a datagram socket has no connection to reset.
fn to_udp_error(err: &io::Error) -> FilesystemError {
    match to_error(err) {
        FilesystemError::ConnectionReset => FilesystemError::ConnectionRefused,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::net::UdpSocket;

    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::net::socket::testing::wait_for;
    use crate::net::HostInet;
    use crate::security::prepare_creds_within;

    /// A socket connected to `addr` through the host.
    fn connected_socket(addr: SocketAddr) -> Arc<UdpSock> {
        let sock =
            UdpSock::new(&prepare_creds_within(&[], None)).expect("an unbounded socket is made");
        let family: Arc<dyn ProtocolFamily> = Arc::new(HostInet);
        sock.connect(&family, addr, addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        sock
    }

    /// A socket connected to a peer that has sent it one datagram.
    fn readable_socket() -> (Arc<UdpSock>, UdpSocket) {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = connected_socket(peer.local_addr().unwrap());
        let host = sock
            .state()
            .host
            .as_ref()
            .map(|(socket, _)| socket.local_addr());
        peer.send_to(b"hello", host.unwrap().unwrap()).unwrap();
        wait_for(|table| sock.poll(table), PollMask::IN);
        (sock, peer)
    }

    // @behavior NW-125
    #[test]
    fn a_udp_socket_holds_its_hop_limit_and_receive_buffer() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = peer.local_addr().unwrap();
        let sock =
            UdpSock::new(&prepare_creds_within(&[], None)).expect("an unbounded socket is made");
        let get = |opt| sock.getsockopt(opt).unwrap() as usize;
        let defaults = (
            get(SockOpt::Ttl),
            get(SockOpt::RcvBuf),
            get(SockOpt::SndBuf),
        );
        let zero = sock.setsockopt(SockOpt::Ttl, 0);
        sock.setsockopt(SockOpt::Ttl, 9)
            .expect("a hop limit Linux takes");
        sock.setsockopt(SockOpt::RcvBuf, 1000).unwrap();
        sock.setsockopt(SockOpt::SndBuf, 1000).unwrap();
        let family: Arc<dyn ProtocolFamily> = Arc::new(HostInet);
        sock.connect(&family, addr, addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        let host = sock
            .state()
            .host
            .as_ref()
            .map(|(socket, _)| (socket.ttl().unwrap(), socket.local_addr().unwrap()));
        let (host_ttl, local) = host.expect("the socket holds its host socket");
        for _ in 0..4 {
            peer.send_to(&[1; 1000], local).unwrap();
        }
        // Loopback hands datagrams over a moment apart, so wait for the two
        // the buffer fits, then give the rest time to arrive and be dropped.
        let started = Instant::now();
        while sock.state().receive_queue.len() < 2 && started.elapsed() < Duration::from_secs(5) {
            sock.on_event();
            thread::sleep(Duration::from_millis(1));
        }
        thread::sleep(Duration::from_millis(50));
        sock.on_event();
        let held = sock.state().receive_queue.len();
        assert_eq!(
            (
                defaults,
                zero,
                host_ttl,
                get(SockOpt::RcvBuf),
                get(SockOpt::SndBuf),
                held
            ),
            (
                (64, MAX_SOCKET_BUFFER, MAX_SOCKET_BUFFER),
                Err(FilesystemError::InvalidInput),
                9,
                2304,
                4608,
                2
            ),
            "a UDP socket must read Linux's default hop limit, refuse zero with EINVAL, give the \
             host socket it connects the limit set, and drop datagrams past a receive buffer \
             doubled as Linux doubles it"
        );
    }

    // @behavior NW-148
    #[test]
    fn empty_datagrams_fill_the_receive_buffer_too() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = peer.local_addr().unwrap();
        let sock =
            UdpSock::new(&prepare_creds_within(&[], None)).expect("an unbounded socket is made");
        sock.setsockopt(SockOpt::RcvBuf, 1000).unwrap();
        let family: Arc<dyn ProtocolFamily> = Arc::new(HostInet);
        sock.connect(&family, addr, addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        let local = sock
            .state()
            .host
            .as_ref()
            .map(|(socket, _)| socket.local_addr().unwrap())
            .expect("the socket holds its host socket");
        for _ in 0..200 {
            peer.send_to(&[], local).unwrap();
        }
        let started = Instant::now();
        while sock.state().receive_queue.len() < 41 && started.elapsed() < Duration::from_secs(5) {
            sock.on_event();
            thread::sleep(Duration::from_millis(1));
        }
        thread::sleep(Duration::from_millis(50));
        sock.on_event();
        assert_eq!(
            sock.state().receive_queue.len(),
            41,
            "a UDP socket whose receive buffer reads 2304 must queue no more than 41 of 200 \
             empty datagrams, as each spends the queue entry that holds it and its sender, as \
             Linux's truesize counts the sk_buff"
        );
    }

    // @behavior NW-152
    #[test]
    fn a_udp_shutdown_keeps_reads_nonblocking_and_refuses_sends_as_linux() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = connected_socket(peer.local_addr().unwrap());
        let unconnected =
            UdpSock::new(&prepare_creds_within(&[], None)).expect("an unbounded socket is made");
        let before = sock.recvmsg(&mut [0; 4]);
        sock.shutdown(Shutdown::Read)
            .expect("a connected socket shuts down");
        let read = (
            sock.recvmsg(&mut [0; 4]),
            sock.poll(None).contains(PollMask::IN),
        );
        sock.shutdown(Shutdown::Write)
            .expect("a connected socket shuts down");
        let sent = sock.sendmsg(b"late");
        assert_eq!(
            (
                before,
                read,
                sent,
                sock.poll(None).contains(PollMask::HUP),
                unconnected.shutdown(Shutdown::Both)
            ),
            (
                Err(FilesystemError::WouldBlock),
                (Err(FilesystemError::WouldBlock), true),
                Err(FilesystemError::BrokenPipe),
                true,
                Err(FilesystemError::NotConnected)
            ),
            "a non-blocking UDP socket whose reading side is shut must poll readable yet still \
             answer EAGAIN on an empty queue, answer EPIPE to a send once its writing side is \
             shut, poll hung up once both are, and answer ENOTCONN to a shutdown with no peer, \
             as Linux's inet_shutdown and __skb_recv_udp"
        );
    }

    // @behavior NW-086
    #[test]
    fn a_zero_byte_read_keeps_the_datagram() {
        let (sock, _peer) = readable_socket();
        let mut file = crate::net::sock_alloc_file(sock.clone(), &Arc::default());
        let mut buf = [0; 8];
        let zero = file.read(&mut []);
        let next = file.read(&mut buf).map(|n| buf[..n].to_vec());
        assert_eq!(
            (zero, next),
            (Ok(0), Ok(b"hello".to_vec())),
            "File::read of zero bytes from a UDP socket holding a datagram must answer zero and \
             leave the datagram for the next read"
        );
    }

    // @behavior NW-087
    #[test]
    fn a_failure_answers_before_the_datagram_it_holds() {
        let (sock, _peer) = readable_socket();
        sock.state().inet.error = Some(FilesystemError::ConnectionRefused);
        let polled = sock.poll(None);
        let mut buf = [0; 8];
        let first = sock.recvmsg(&mut buf);
        let second = sock.recvmsg(&mut buf).map(|n| buf[..n].to_vec());
        assert_eq!(
            (polled, first, second),
            (
                PollMask::IN | PollMask::OUT | PollMask::ERR,
                Err(FilesystemError::ConnectionRefused),
                Ok(b"hello".to_vec())
            ),
            "a UDP socket holding a datagram and a failure must poll IN and ERR, then answer the \
             failure before the datagram, as __skb_recv_udp asks sock_error first"
        );
    }

    /// Stop the reactor taking `sock`'s datagrams in, so each stays in the
    /// host's queue until the socket itself takes it; the host socket's own
    /// address.
    fn stop_reactor_for(sock: &UdpSock) -> SocketAddr {
        let local = {
            let mut state = sock.state();
            let (socket, registration) = state.host.as_mut().expect("a connected socket");
            registration.deregister(socket);
            socket.local_addr().unwrap()
        };
        // An event the reactor took before the deregister may still run.
        thread::sleep(Duration::from_millis(100));
        local
    }

    /// Wait until a datagram waits in `sock`'s host socket.
    fn wait_until_queued(sock: &UdpSock) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            let state = sock.state();
            let (socket, _) = state.host.as_ref().expect("a connected socket");
            if socket.peek_from(&mut [0; 8]).is_ok() {
                return;
            }
            drop(state);
            thread::sleep(Duration::from_millis(1));
        }
        panic!("the peer's datagram never reached the host socket's queue within 5 s");
    }

    // @behavior NW-174
    #[test]
    fn a_flooded_udp_socket_takes_its_datagrams_in_bounded_turns() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = connected_socket(peer.local_addr().unwrap());
        let local = stop_reactor_for(&sock);
        for i in 0..100u8 {
            peer.send_to(&[i], local).unwrap();
        }
        wait_until_queued(&sock);
        thread::sleep(Duration::from_millis(200));
        UdpSock::pump(&mut sock.state());
        let one_turn = sock.state().receive_queue.len();
        let mut read = Vec::new();
        let mut buf = [0; 1];
        while let Ok((1, _)) = sock.recvfrom(&mut buf) {
            read.push(buf[0]);
        }
        assert_eq!(
            (one_turn, read),
            (64, (0..100u8).collect::<Vec<_>>()),
            "a UDP socket whose peer sent 100 datagrams must take 64 in one turn, as \
             NAPI_POLL_WEIGHT bounds one poll, and read all 100 in order as each read takes in \
             what still waits"
        );
    }

    // @behavior NW-172
    #[test]
    fn a_datagram_reads_with_the_peer_that_sent_it() {
        let first = UdpSocket::bind("127.0.0.1:0").unwrap();
        let second = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (at_first, at_second) = (first.local_addr().unwrap(), second.local_addr().unwrap());
        let sock = connected_socket(at_first);
        let local = stop_reactor_for(&sock);
        first.send_to(b"one", local).unwrap();
        wait_until_queued(&sock);
        let family: Arc<dyn ProtocolFamily> = Arc::new(HostInet);
        sock.connect(&family, at_second, at_second, &OpenConnections::new(None))
            .expect("an uncapped socket connects again");
        second.send_to(b"two", local).unwrap();
        wait_until_queued(&sock);
        UdpSock::pump(&mut sock.state());
        let mut buf = [0; 8];
        let mut read = || {
            sock.recvfrom(&mut buf)
                .map(|(n, from)| (buf[..n].to_vec(), from))
        };
        assert_eq!(
            (read(), read()),
            (
                Ok((b"one".to_vec(), at_first)),
                Ok((b"two".to_vec(), at_second))
            ),
            "UdpSock::recvfrom after the socket connects to a second peer must read the first \
             peer's datagram with the first peer and the second's with the second, as \
             udp_recvmsg names each datagram's source"
        );
    }

    // @behavior NW-172
    #[test]
    fn a_datagram_waiting_as_the_socket_moves_family_reads_with_its_peer() {
        let first = UdpSocket::bind("127.0.0.1:0").unwrap();
        let second = UdpSocket::bind("[::1]:0").unwrap();
        let (at_first, at_second) = (first.local_addr().unwrap(), second.local_addr().unwrap());
        let sock = connected_socket(at_first);
        let local = stop_reactor_for(&sock);
        first.send_to(b"one", local).unwrap();
        wait_until_queued(&sock);
        let family: Arc<dyn ProtocolFamily> = Arc::new(HostInet);
        sock.connect(&family, at_second, at_second, &OpenConnections::new(None))
            .expect("an uncapped socket connects to an IPv6 peer");
        let mut buf = [0; 8];
        let read = sock
            .recvfrom(&mut buf)
            .map(|(n, from)| (buf[..n].to_vec(), from));
        assert_eq!(
            read,
            Ok((b"one".to_vec(), at_first)),
            "a UDP socket that connects from an IPv4 peer to an IPv6 one must still read the \
             datagram the first peer sent before, with the first peer, as Linux's repointed \
             socket keeps its receive queue"
        );
    }

    // @behavior NW-088
    #[test]
    fn a_socket_connected_again_keeps_its_place() {
        let first = UdpSocket::bind("127.0.0.1:0").unwrap();
        let second = UdpSocket::bind("127.0.0.1:0").unwrap();
        let family: Arc<dyn ProtocolFamily> = Arc::new(HostInet);
        let cap = OpenConnections::new(Some(1));
        let sock =
            UdpSock::new(&prepare_creds_within(&[], None)).expect("an unbounded socket is made");
        let at = |peer: &UdpSocket| peer.local_addr().unwrap();
        let heard_by = |peer: &UdpSocket| {
            wait_for(|table| sock.poll(table), PollMask::OUT);
            sock.sendmsg(b"ping").unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            peer.recv_from(&mut [0; 8])
                .map(|(_, from)| from.port())
                .ok()
        };
        let once = sock.connect(&family, at(&first), at(&first), &cap);
        let first_heard = heard_by(&first);
        let again = sock.connect(&family, at(&second), at(&second), &cap);
        let second_heard = heard_by(&second);
        sock.release();
        assert_eq!(
            (
                once,
                again,
                first_heard.is_some() && first_heard == second_heard
            ),
            (Ok(()), Ok(()), true),
            "UdpSock::connect to a second peer under a cap of one must keep the place the \
             socket holds, not answer EAGAIN, and the second peer must hear from the port the \
             first heard"
        );
    }

    // @behavior NW-053
    #[test]
    fn a_datagram_round_trips_with_one_peer() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = connected_socket(peer.local_addr().unwrap());
        wait_for(|table| sock.poll(table), PollMask::OUT);
        sock.sendmsg(b"ping").unwrap();
        let mut heard = [0; 16];
        let (n, from) = peer.recv_from(&mut heard).unwrap();
        peer.send_to(b"PONG", from).unwrap();
        wait_for(|table| sock.poll(table), PollMask::IN);
        let mut answer = [0; 2];
        let read = sock.recvmsg(&mut answer).unwrap();
        let rest = sock.recvmsg(&mut [0; 2]);
        sock.release();
        assert_eq!(
            (&heard[..n], &answer[..read], rest),
            (&b"ping"[..], &b"PO"[..], Err(FilesystemError::WouldBlock)),
            "a UDP socket must send a datagram to its peer and read one back per read, cut to the \
             buffer, with the rest of it gone"
        );
    }

    // @behavior NW-070
    #[test]
    fn a_reset_the_host_reports_on_udp_reads_as_refused() {
        let reset = io::Error::from(io::ErrorKind::ConnectionReset);
        assert_eq!(
            to_udp_error(&reset),
            FilesystemError::ConnectionRefused,
            "a UDP socket's failure the host reports as a reset, as Windows reports an ICMP port \
             unreachable, must answer ECONNREFUSED, as Linux's does"
        );
    }

    // @behavior NW-070
    #[test]
    fn a_udp_failure_is_answered_once() {
        let closed = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = closed.local_addr().unwrap();
        drop(closed);
        let sock = connected_socket(addr);
        wait_for(|table| sock.poll(table), PollMask::OUT);
        sock.sendmsg(b"ping").unwrap();
        wait_for(|table| sock.poll(table), PollMask::ERR);
        let first = sock.recvmsg(&mut [0; 4]);
        let again = sock.recvmsg(&mut [0; 4]);
        sock.release();
        assert_eq!(
            (first, again),
            (
                Err(FilesystemError::ConnectionRefused),
                Err(FilesystemError::WouldBlock)
            ),
            "UdpSock::recvmsg after a datagram to a closed port must answer ECONNREFUSED once \
             and then read on, as sock_error clears sk_err"
        );
    }

    #[test]
    fn a_datagram_past_what_udp_carries_is_refused() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = connected_socket(peer.local_addr().unwrap());
        wait_for(|table| sock.poll(table), PollMask::OUT);
        let answer = sock.sendmsg(&vec![0; MAX_DATAGRAM + 1]);
        sock.release();
        assert_eq!(
            answer,
            Err(FilesystemError::MessageTooLong),
            "UdpSock::sendmsg of a datagram past what IPv4 UDP carries must answer EMSGSIZE, as \
             udp_sendmsg does"
        );
    }
}
