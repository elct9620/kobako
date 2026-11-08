//! A UDP socket the guest holds, connected to one peer, as Linux's `udp_sock`
//! behind a socket file. The host socket is non-blocking and the reactor
//! watches it: each change reads what arrived into the receive queue and sends
//! what the send queue holds, then wakes the socket's wait queue. A read takes
//! one datagram, as `recv` on a datagram socket does.

use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::connections::{open, OpenConnections, Place};
use super::family::ProtocolFamily;
use super::reactor::{Reactor, Ready, Registration};
use super::socket::{to_error, ProtoOps};
use super::SOCKET_BUFFER;
use crate::vfs::{FilesystemError, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// The most one IPv4 datagram carries.
pub const MAX_DATAGRAM: usize = 65_507;

struct State {
    /// The host socket, and its place in the reactor, until released.
    host: Option<(mio::net::UdpSocket, Registration)>,
    failed: Option<FilesystemError>,
    received: VecDeque<Vec<u8>>,
    received_bytes: usize,
    sending: VecDeque<Vec<u8>>,
    sending_bytes: usize,
    local: Option<SocketAddr>,
    /// The socket's place against its Sandbox's cap and the process's.
    place: Option<Place>,
}

/// One UDP socket, shared by the socket file and the reactor.
pub struct UdpSock {
    state: Mutex<State>,
    wait: Arc<WaitQueueHead>,
}

impl UdpSock {
    /// A socket connected to `addr` through `family`, its host socket watched
    /// by the reactor; it counts against `open_connections` and the process until
    /// released, and a full count answers `EAGAIN`.
    pub fn connect(
        family: Arc<dyn ProtocolFamily>,
        addr: SocketAddr,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<Arc<Self>, FilesystemError> {
        let place = Some(open(open_connections).ok_or(FilesystemError::WouldBlock)?);
        let reactor = Reactor::current().map_err(|err| to_error(&err))?;
        let mut socket = mio::net::UdpSocket::from_std(
            family
                .connect_datagram(addr)
                .map_err(|err| to_error(&err))?,
        );
        let local = socket.local_addr().ok();
        let mut failed = None;
        let sock = Arc::new_cyclic(|this: &std::sync::Weak<Self>| {
            let ready: std::sync::Weak<dyn Ready> = this.clone();
            let host = match reactor.register(&mut socket, ready) {
                Ok(registration) => Some((socket, registration)),
                Err(err) => {
                    failed = Some(to_error(&err));
                    None
                }
            };
            Self {
                state: Mutex::new(State {
                    host,
                    failed: None,
                    received: VecDeque::new(),
                    received_bytes: 0,
                    sending: VecDeque::new(),
                    sending_bytes: 0,
                    local,
                    place,
                }),
                wait: Arc::default(),
            }
        });
        match failed {
            Some(err) => Err(err),
            None => Ok(sock),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read every datagram that arrived and send what is queued, without
    /// blocking; a full receive queue drops what comes, as a full `sk_rcvbuf`
    /// does.
    fn pump(state: &mut State) {
        let Some((socket, _)) = state.host.as_ref() else {
            return;
        };
        let mut datagram = vec![0; MAX_DATAGRAM];
        while state.failed.is_none() {
            match socket.recv(&mut datagram) {
                Ok(n) => {
                    if state.received_bytes + n <= SOCKET_BUFFER {
                        state.received.push_back(datagram[..n].to_vec());
                        state.received_bytes += n;
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => state.failed = Some(to_error(&err)),
            }
        }
        while let Some(datagram) = state.sending.front() {
            match socket.send(datagram) {
                Ok(_) => {
                    state.sending_bytes -= datagram.len();
                    state.sending.pop_front();
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => {
                    state.failed = Some(to_error(&err));
                    break;
                }
            }
        }
    }

    fn changed(&self, mut state: MutexGuard<'_, State>) {
        Self::pump(&mut state);
        drop(state);
        self.wait.wake_up();
    }

    /// The host's own end, once the socket opened.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.state().local
    }
}

impl Ready for UdpSock {
    fn ready(&self) {
        let state = self.state();
        self.changed(state);
    }
}

/// UDP's answers for its socket file, as `inet_dgram_ops` hands each call to
/// `udp_prot`.
impl ProtoOps for UdpSock {
    /// One datagram, cut to `buf`, as a datagram socket's read takes one. A
    /// failure is answered once and then cleared, as `sock_error` swaps
    /// `sk_err` with zero, so the socket reads on.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        let Some(datagram) = state.received.pop_front() else {
            return Err(state.failed.take().unwrap_or(FilesystemError::WouldBlock));
        };
        state.received_bytes -= datagram.len();
        let n = buf.len().min(datagram.len());
        buf[..n].copy_from_slice(&datagram[..n]);
        Ok(n)
    }

    /// One datagram, sent whole as the host socket takes it; one past what UDP
    /// carries is refused, and a full queue answers `WouldBlock`.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        if buf.len() > MAX_DATAGRAM {
            return Err(FilesystemError::InvalidInput);
        }
        let mut state = self.state();
        if let Some(err) = state.failed.take() {
            return Err(err);
        }
        if state.host.is_none() || state.sending_bytes + buf.len() > SOCKET_BUFFER {
            return Err(FilesystemError::WouldBlock);
        }
        state.sending.push_back(buf.to_vec());
        state.sending_bytes += buf.len();
        self.changed(state);
        Ok(buf.len())
    }

    /// As `udp_poll` answers.
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.wait, table);
        let state = self.state();
        let mut mask = PollMask::default();
        if !state.received.is_empty() || state.failed.is_some() {
            mask = mask | PollMask::IN;
        }
        if state.host.is_some() && state.sending_bytes < SOCKET_BUFFER {
            mask = mask | PollMask::OUT;
        }
        if state.failed.is_some() {
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

    // @behavior NW-053
    #[test]
    fn a_datagram_round_trips_with_one_peer() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = UdpSock::connect(
            Arc::new(HostInet),
            peer.local_addr().unwrap(),
            &OpenConnections::new(None),
        )
        .expect("the reactor starts");
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
        let sock = UdpSock::connect(Arc::new(HostInet), addr, &OpenConnections::new(None)).unwrap();
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
        let sock = UdpSock::connect(
            Arc::new(HostInet),
            peer.local_addr().unwrap(),
            &OpenConnections::new(None),
        )
        .unwrap();
        until(&sock, PollMask::OUT);
        let answer = sock.sendmsg(&vec![0; MAX_DATAGRAM + 1]);
        sock.release();
        assert_eq!(
            answer,
            Err(FilesystemError::InvalidInput),
            "UdpSock::sendmsg of a datagram past what IPv4 UDP carries must be refused"
        );
    }
}
