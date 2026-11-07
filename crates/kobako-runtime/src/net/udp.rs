//! A UDP socket the guest holds, connected to one peer, as Linux's `udp_sock`
//! behind a socket file. Worker threads do the host I/O, as a reactor would:
//! one opens the host socket and reads datagrams into the receive queue, one
//! writes the send queue out, and each wakes the socket's wait queue when what
//! a poll finds changes. A read takes one datagram, as `recv` on a datagram
//! socket does.

use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::family::{Datagram, ProtocolFamily};
use super::socket::{to_error, ProtoOps};
use super::workers::{charge, Workers};
use super::SOCKET_BUFFER;
use crate::vfs::{FilesystemError, PollMask};
use crate::wait::WaitQueueHead;

/// The most one IPv4 datagram carries.
pub const MAX_DATAGRAM: usize = 65_507;

/// How long a receive waits before the reader looks again whether the socket
/// was let go, since a datagram socket has no shutdown that wakes it.
const RECEIVE_SLICE: Duration = Duration::from_millis(50);

struct State {
    open: bool,
    failed: Option<FilesystemError>,
    received: VecDeque<Vec<u8>>,
    received_bytes: usize,
    sending: VecDeque<Vec<u8>>,
    sending_bytes: usize,
    local: Option<SocketAddr>,
    released: bool,
}

/// One UDP socket, shared by the socket file and its workers.
pub struct UdpSock {
    state: Mutex<State>,
    writer: Condvar,
    wait: Arc<WaitQueueHead>,
    deadline: Option<Instant>,
}

impl UdpSock {
    /// A socket connected to `addr` through `family`, its host socket opened on
    /// a worker so this returns at once. Its two workers count against
    /// `workers` and the process, and a full count answers `EAGAIN`.
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
                open: false,
                failed: None,
                received: VecDeque::new(),
                received_bytes: 0,
                sending: VecDeque::new(),
                sending_bytes: 0,
                local: None,
                released: false,
            }),
            writer: Condvar::new(),
            wait: Arc::default(),
            deadline,
        });
        let worker = sock.clone();
        thread::spawn(move || {
            let _reader = reader;
            let socket = match family.connect_datagram(addr) {
                Ok(socket) => Arc::<dyn Datagram>::from(socket),
                Err(err) => {
                    worker.state().failed = Some(to_error(&err));
                    worker.changed();
                    return;
                }
            };
            {
                let mut state = worker.state();
                state.local = socket.local_addr().ok();
                state.open = true;
            }
            worker.changed();
            let sender = worker.clone();
            let outgoing = socket.clone();
            thread::spawn(move || {
                let _writer = writer;
                sender.write_worker(&*outgoing);
            });
            worker.read_worker(&*socket);
        });
        Ok(sock)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn changed(&self) {
        self.writer.notify_all();
        self.wait.wake_up();
    }

    fn expired(&self) -> bool {
        self.deadline.is_some_and(|at| Instant::now() >= at)
    }

    /// Read datagrams into the receive queue while it has room, until the
    /// socket is let go or the deadline passes; a full queue drops what comes,
    /// as a full `sk_rcvbuf` does.
    fn read_worker(&self, socket: &dyn Datagram) {
        let _ = socket.set_read_timeout(Some(RECEIVE_SLICE));
        let mut datagram = vec![0; MAX_DATAGRAM];
        loop {
            if self.state().released || self.expired() {
                return;
            }
            match socket.recv(&mut datagram) {
                Ok(n) => {
                    let mut state = self.state();
                    if state.received_bytes + n <= SOCKET_BUFFER {
                        state.received.push_back(datagram[..n].to_vec());
                        state.received_bytes += n;
                    }
                    drop(state);
                    self.changed();
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(err) => {
                    self.state().failed = Some(to_error(&err));
                    self.changed();
                    return;
                }
            }
        }
    }

    /// Write the send queue out as it fills, until the socket is let go.
    fn write_worker(&self, socket: &dyn Datagram) {
        loop {
            let datagram = {
                let mut state = self.state();
                while state.sending.is_empty() && !state.released {
                    state = self
                        .writer
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if state.released {
                    return;
                }
                let datagram = state.sending.pop_front().expect("the queue holds one");
                state.sending_bytes -= datagram.len();
                datagram
            };
            if let Err(err) = socket.send(&datagram) {
                self.state().failed = Some(to_error(&err));
            }
            self.changed();
        }
    }

    /// The host's own end, once the socket opened.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.state().local
    }
}

/// UDP's answers for its socket file, as `inet_dgram_ops` hands each call to
/// `udp_prot`.
impl ProtoOps for UdpSock {
    /// One datagram, cut to `buf`, as a datagram socket's read takes one.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut state = self.state();
        let Some(datagram) = state.received.pop_front() else {
            return Err(state.failed.clone().unwrap_or(FilesystemError::WouldBlock));
        };
        state.received_bytes -= datagram.len();
        let n = buf.len().min(datagram.len());
        buf[..n].copy_from_slice(&datagram[..n]);
        Ok(n)
    }

    /// One datagram, queued whole for the writer; one past what UDP carries is
    /// refused, and a full queue answers `WouldBlock`.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        if buf.len() > MAX_DATAGRAM {
            return Err(FilesystemError::InvalidInput);
        }
        let mut state = self.state();
        if let Some(err) = state.failed.clone() {
            return Err(err);
        }
        if !state.open || state.sending_bytes + buf.len() > SOCKET_BUFFER {
            return Err(FilesystemError::WouldBlock);
        }
        state.sending.push_back(buf.to_vec());
        state.sending_bytes += buf.len();
        drop(state);
        self.changed();
        Ok(buf.len())
    }

    fn poll_mask(&self) -> PollMask {
        let state = self.state();
        let mut mask = PollMask::default();
        if !state.received.is_empty() || state.failed.is_some() {
            mask = mask | PollMask::IN;
        }
        if state.open && state.sending_bytes < SOCKET_BUFFER {
            mask = mask | PollMask::OUT;
        }
        if state.failed.is_some() {
            mask = mask | PollMask::ERR;
        }
        mask
    }

    fn wait_queue(&self) -> &Arc<WaitQueueHead> {
        &self.wait
    }

    /// The workers end: the writer at once, the reader within a slice.
    fn release(&self) {
        self.state().released = true;
        self.changed();
    }
}

#[cfg(test)]
mod tests {
    use std::net::UdpSocket;

    use super::*;
    use crate::net::{HostInet, SANDBOX_WORKERS};

    /// Wait until `sock`'s poll finds `mask`.
    fn until(sock: &UdpSock, mask: PollMask) {
        let started = Instant::now();
        while !sock.poll_mask().contains(mask) {
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
            Some(Instant::now() + Duration::from_secs(10)),
            &Workers::new(SANDBOX_WORKERS),
        )
        .expect("a fresh Sandbox has room");
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

    #[test]
    fn a_datagram_past_what_udp_carries_is_refused() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = UdpSock::connect(
            Arc::new(HostInet),
            peer.local_addr().unwrap(),
            None,
            &Workers::new(SANDBOX_WORKERS),
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
