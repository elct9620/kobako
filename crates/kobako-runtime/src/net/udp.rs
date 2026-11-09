//! A UDP socket the guest holds, connected to one peer, as Linux's `udp_sock`
//! behind a socket file. The host socket is non-blocking and the reactor
//! watches it: each change reads what arrived into the receive queue, then
//! wakes the socket's wait queue. A send goes to the host socket at once, as
//! `udp_sendmsg` sends, and a read takes one datagram, as `recv` on a
//! datagram socket does.

use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super::connections::{open, OpenConnections, Place};
use super::family::ProtocolFamily;
use super::reactor::{Reactor, Ready, Registration};
use super::socket::{to_error, ProtoOps};
use super::MAX_SOCKET_BUFFER;
use crate::vfs::{FilesystemError, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// The most one IPv4 datagram carries.
pub const MAX_DATAGRAM: usize = 65_507;

struct State {
    /// The host socket, and its place in the reactor, until released.
    host: Option<(mio::net::UdpSocket, Registration)>,
    /// The socket's pending failure, as `sk_err`: answered once, then cleared.
    error: Option<FilesystemError>,
    received: VecDeque<Vec<u8>>,
    received_bytes: usize,
    /// Whether the host socket took the last send, until the reactor next
    /// sees it change, as `sock_writeable` reads its room.
    writable: bool,
    /// The port the guest reads for its own end, which its namespace gave as
    /// it first connected, as a NAT's tuple pairs it with the host's.
    virtual_port: Option<u16>,
    /// The peer as the guest named it, a virtual address standing for its
    /// name, as cgroup `getpeername4` maps a real peer back.
    named: Option<SocketAddr>,
    /// The socket's place against its Sandbox's cap and the process's.
    place: Option<Place>,
}

/// One UDP socket, shared by the socket file and the reactor.
pub struct UdpSock {
    state: Mutex<State>,
    wait: Arc<WaitQueueHead>,
    this: Weak<Self>,
}

impl UdpSock {
    /// A socket connected to nothing, as `inet_create` makes one.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            state: Mutex::new(State {
                host: None,
                error: None,
                received: VecDeque::new(),
                received_bytes: 0,
                writable: true,
                virtual_port: None,
                named: None,
                place: None,
            }),
            wait: Arc::default(),
            this: this.clone(),
        })
    }

    /// Connect to `addr` through `family`, the guest having named `named`,
    /// the host socket watched by the reactor; it counts against
    /// `open_connections` and the process until released, and a full count
    /// answers `EAGAIN`. Connecting again moves the peer and keeps the place,
    /// as `__ip4_datagram_connect` repoints a socket.
    pub(crate) fn connect(
        &self,
        family: &Arc<dyn ProtocolFamily>,
        addr: SocketAddr,
        named: SocketAddr,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<(), FilesystemError> {
        let place = match self.state().place.is_some() {
            true => None,
            false => Some(open(open_connections).ok_or(FilesystemError::WouldBlock)?),
        };
        let reactor = Reactor::current().map_err(|err| to_error(&err))?;
        let mut socket = mio::net::UdpSocket::from_std(
            family
                .connect_datagram(addr)
                .map_err(|err| to_error(&err))?,
        );
        let ready: Weak<dyn Ready> = self.this.clone();
        let registration = reactor
            .register(&mut socket, ready)
            .map_err(|err| to_error(&err))?;
        let mut state = self.state();
        if let Some((mut earlier, registration)) = state.host.take() {
            registration.deregister(&mut earlier);
        }
        state.host = Some((socket, registration));
        state.named = Some(named);
        state.writable = true;
        if let Some(place) = place {
            state.place = Some(place);
        }
        Ok(())
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read every datagram that arrived, without blocking; a full receive
    /// queue drops what comes, as a full `sk_rcvbuf` does. A failure the host
    /// reports is kept pending and the queue reads on behind it, as datagrams
    /// queue behind `sk_err`.
    fn pump(state: &mut State) {
        let Some((socket, _)) = state.host.as_ref() else {
            return;
        };
        let mut datagram = vec![0; MAX_DATAGRAM];
        loop {
            match socket.recv(&mut datagram) {
                Ok(n) => {
                    if state.received_bytes + n <= MAX_SOCKET_BUFFER {
                        state.received.push_back(datagram[..n].to_vec());
                        state.received_bytes += n;
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => state.error = Some(to_error(&err)),
            }
        }
    }

    fn pump_and_wake(&self, mut state: MutexGuard<'_, State>) {
        Self::pump(&mut state);
        drop(state);
        self.wait.wake_up();
    }

    /// The port the guest reads for its own end, once it connected.
    pub fn virtual_port(&self) -> Option<u16> {
        self.state().virtual_port
    }

    /// Hold the port `alloc` gives as the guest's own, unless one is held, so
    /// a socket connected again keeps its port.
    pub(crate) fn hold_virtual_port(&self, alloc: impl FnOnce() -> u16) {
        self.state().virtual_port.get_or_insert_with(alloc);
    }

    /// The peer as the guest named it, once connected, as cgroup
    /// `getpeername4` maps the real peer back.
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.state().named
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
    /// One datagram, cut to `buf`, as a datagram socket's read takes one. A
    /// pending failure is answered first, once, and then cleared, as
    /// `__skb_recv_udp` asks `sock_error` before it dequeues, so the socket
    /// reads on.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        if let Some(err) = state.error.take() {
            return Err(err);
        }
        let Some(datagram) = state.received.pop_front() else {
            return Err(FilesystemError::WouldBlock);
        };
        state.received_bytes -= datagram.len();
        let n = buf.len().min(datagram.len());
        buf[..n].copy_from_slice(&datagram[..n]);
        Ok(n)
    }

    /// One datagram, sent whole and at once, as `udp_sendmsg` sends: one past
    /// what UDP carries answers EMSGSIZE, one with no peer to go to
    /// EDESTADDRREQ, and a pending failure is answered once first. A host
    /// socket with no room answers `WouldBlock` and sends nothing, so no
    /// datagram is taken that a release could lose.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        if buf.len() > MAX_DATAGRAM {
            return Err(FilesystemError::MessageTooLong);
        }
        let mut state = self.state();
        if let Some(err) = state.error.take() {
            return Err(err);
        }
        if state.named.is_none() {
            return Err(FilesystemError::DestinationAddressRequired);
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
            Err(err) => Err(to_error(&err)),
        }
    }

    /// As `datagram_poll` answers: readable once a datagram waits, writable
    /// while the host socket takes a send, and failed while a failure is
    /// pending.
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.wait, table);
        let state = self.state();
        let mut mask = PollMask::default();
        if !state.received.is_empty() {
            mask = mask | PollMask::IN;
        }
        if (state.host.is_some() && state.writable) || state.named.is_none() {
            mask = mask | PollMask::OUT;
        }
        if state.error.is_some() {
            mask = mask | PollMask::ERR;
        }
        mask
    }

    /// Let the host socket go, deregistered from the reactor.
    fn release(&self) {
        let mut state = self.state();
        state.place = None;
        if let Some((mut socket, registration)) = state.host.take() {
            registration.deregister(&mut socket);
        }
        drop(state);
        self.wait.wake_up();
    }
}

#[cfg(test)]
mod tests {
    use std::net::UdpSocket;

    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::net::HostInet;

    /// A socket connected to `addr` through the host.
    fn connected(addr: SocketAddr) -> Arc<UdpSock> {
        let sock = UdpSock::new();
        let family: Arc<dyn ProtocolFamily> = Arc::new(HostInet);
        sock.connect(&family, addr, addr, &OpenConnections::new(None))
            .expect("an uncapped socket connects");
        sock
    }

    /// Wait until `sock`'s poll finds `mask`.
    fn until(sock: &UdpSock, mask: PollMask) {
        let started = Instant::now();
        while !sock.poll(None).contains(mask) {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the socket never became ready"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// A socket connected to a peer that has sent it one datagram.
    fn holding_a_datagram() -> (Arc<UdpSock>, UdpSocket) {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = connected(peer.local_addr().unwrap());
        let host = sock
            .state()
            .host
            .as_ref()
            .map(|(socket, _)| socket.local_addr());
        peer.send_to(b"hello", host.unwrap().unwrap()).unwrap();
        until(&sock, PollMask::IN);
        (sock, peer)
    }

    // @behavior NW-086
    #[test]
    fn a_zero_byte_read_keeps_the_datagram() {
        let (sock, _peer) = holding_a_datagram();
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
        let (sock, _peer) = holding_a_datagram();
        sock.state().error = Some(FilesystemError::ConnectionRefused);
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

    // @behavior NW-088
    #[test]
    fn a_socket_connected_again_keeps_its_place() {
        let first = UdpSocket::bind("127.0.0.1:0").unwrap();
        let second = UdpSocket::bind("127.0.0.1:0").unwrap();
        let family: Arc<dyn ProtocolFamily> = Arc::new(HostInet);
        let cap = OpenConnections::new(Some(1));
        let sock = UdpSock::new();
        let at = |peer: &UdpSocket| peer.local_addr().unwrap();
        let once = sock.connect(&family, at(&first), at(&first), &cap);
        let again = sock.connect(&family, at(&second), at(&second), &cap);
        sock.release();
        assert_eq!(
            (once, again),
            (Ok(()), Ok(())),
            "UdpSock::connect to a second peer under a cap of one must keep the place the \
             socket holds, not answer EAGAIN"
        );
    }

    // @behavior NW-053
    #[test]
    fn a_datagram_round_trips_with_one_peer() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = connected(peer.local_addr().unwrap());
        until(&sock, PollMask::OUT);
        sock.sendmsg(b"ping").unwrap();
        let mut heard = [0; 16];
        let (n, from) = peer.recv_from(&mut heard).unwrap();
        peer.send_to(b"PONG", from).unwrap();
        until(&sock, PollMask::IN);
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
    fn a_udp_failure_is_answered_once() {
        let closed = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = closed.local_addr().unwrap();
        drop(closed);
        let sock = connected(addr);
        until(&sock, PollMask::OUT);
        sock.sendmsg(b"ping").unwrap();
        until(&sock, PollMask::ERR);
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
        let sock = connected(peer.local_addr().unwrap());
        until(&sock, PollMask::OUT);
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
