//! The TCP link under an HTTP request: the connector that reaches the one
//! authority a request named, and the connection it opened. The connector
//! dials through a TCP socket of the Invocation's own, as an egress proxy
//! dials through the kernel, so its upstream passes the socket layer's
//! filter, holds a virtual port, and queues within its buffers, as a guest's
//! socket does.

use std::cell::Cell;
use std::fmt;
use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, Shutdown, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{ready, Context, Poll};
use std::time::Instant;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::connections::{ConnectionBudget, Place};
use super::http::HttpError;
use super::lookup::NameLookup;
use super::namespace::TcpSockets;
use super::socket::{BindHash, Destination};
use super::task::PollBudget;
use super::{Namespace, ProtoOps, ProtocolFamily, SockOpt, TcpSock};
use crate::errno::CarriedErrno;
use crate::network::{Host, Protocol, Ruleset};
use crate::wait::WaitQueueHead;
use crate::Errno;

/// The only dialer the outbound seam has: it reaches the one authority the
/// request named, resolving a name on the host and dialing each real address
/// in turn through a socket the ruleset filters.
pub struct Connector {
    port: u16,
    ruleset: Arc<Ruleset>,
    family: Arc<dyn ProtocolFamily>,
    /// Where the dial goes.
    target: Target,
    ports: Arc<BindHash>,
    tcp_sockets: Arc<TcpSockets>,
    /// The request's place, which a lookup of the authority holds too.
    place: Arc<Place>,
    /// The Invocation's connection budget, which each address past the
    /// first spends.
    budget: Arc<ConnectionBudget>,
    /// When a connect opened, which its exchange's wait for the head starts
    /// from.
    pub(crate) connected: Arc<OnceLock<Instant>>,
    /// The queue its exchange's waiters sleep on, woken as the connect opens,
    /// so a wait bound by the connect's timeout takes the head's.
    pub(crate) wait: Option<Arc<WaitQueueHead>>,
    /// What one poll of the exchange's task may move on the connection.
    pub(crate) poll_budget: Arc<PollBudget>,
}

impl Connector {
    /// The connector for `host` at `port` in `net`, holding the request's
    /// `place`, which a lookup of a named host holds too. It spends one of the
    /// connection budget before its lookup or first dial leaves, as a Worker
    /// counts a `fetch` once; a spent budget answers `ConnectionLimitReached`.
    pub(crate) fn new(
        net: &Namespace,
        host: Host,
        port: u16,
        place: Arc<Place>,
    ) -> Result<Self, HttpError> {
        let budget = net.connection_budget().clone();
        budget
            .spend()
            .map_err(|_| HttpError::ConnectionLimitReached)?;
        let target = match host {
            Host::Address(ip) => Target::Address(ip),
            Host::Name(name) => Target::Named {
                lookup: NameLookup::start_holding(
                    net.resolver().clone(),
                    name.clone(),
                    place.clone(),
                ),
                name,
            },
        };
        Ok(Self {
            port,
            ruleset: net.ruleset().clone(),
            family: net.family().clone(),
            target,
            ports: net.ports_of(Protocol::Http).clone(),
            tcp_sockets: net.tcp_sockets().clone(),
            place,
            budget,
            connected: Arc::default(),
            wait: None,
            poll_budget: Arc::default(),
        })
    }

    /// The host and port the request named.
    pub fn authority(&self) -> (String, u16) {
        let host = match &self.target {
            Target::Address(ip) => ip.to_string(),
            Target::Named { name, .. } => name.clone(),
        };
        (host, self.port)
    }

    /// A connection to the authority, once one of its addresses answers. The
    /// connector tries each real address in turn, as an HTTP client's
    /// connector does, while a guest's TCP socket keeps a loop of its own; an
    /// address the filter refuses is passed over, and none admitted is
    /// denied. Each address dialed past the first spends one of the
    /// connection budget as it is about to leave, and a spent budget ends the
    /// dial with `ConnectionLimitReached`.
    pub async fn dial(&self) -> Result<Transport, HttpError> {
        let dests = match &self.target {
            Target::Address(ip) => vec![Destination::Literal(SocketAddr::new(*ip, self.port))],
            Target::Named { lookup, .. } => poll_fn(|cx| lookup.poll_answer(cx))
                .await
                .map_err(|_| HttpError::DnsError)?
                .into_iter()
                .map(|real| Destination::Named {
                    name: lookup.name().to_string(),
                    real,
                    port: self.port,
                })
                .collect(),
        };
        let mut last = HttpError::Denied;
        // The request spent one already, which its first dial takes.
        let prepaid = Cell::new(true);
        for dest in &dests {
            let sock = TcpSock::create_kern(&self.family, &self.tcp_sockets);
            // Set before the dial, so the host socket takes it as it is
            // made, as an HTTP client writes each request whole; a guest's
            // own socket keeps Linux's default.
            sock.setsockopt(SockOpt::NoDelay, 1)
                .map_err(|err| HttpError::from_errno(&err))?;
            let exhausted = Cell::new(false);
            let spend = || match prepaid.replace(false) {
                true => Ok(()),
                false => self.budget.spend().inspect_err(|_| exhausted.set(true)),
            };
            match sock.connect_upstream(&self.ports, &self.ruleset, dest, &self.place, spend) {
                Err(_) if exhausted.get() => return Err(HttpError::ConnectionLimitReached),
                Err(Errno::PermissionDenied) => continue,
                Err(err) => {
                    last = HttpError::from_errno(&err);
                    continue;
                }
                Ok(()) => {}
            }
            let connection = Transport {
                sock,
                poll_budget: self.poll_budget.clone(),
            };
            match connection.wait_for_connect().await {
                Ok(()) => {
                    let _ = self.connected.set(Instant::now());
                    if let Some(wait) = &self.wait {
                        wait.wake_up();
                    }
                    return Ok(connection);
                }
                Err(err) => last = HttpError::from_errno(&err),
            }
        }
        Err(last)
    }
}

/// Where a connector dials: a literal address, or a named authority and its
/// lookup, started on the thread that sends the request, as a guest's own
/// lookup is.
enum Target {
    Address(IpAddr),
    Named {
        name: String,
        lookup: Arc<NameLookup>,
    },
}

impl fmt::Debug for Connector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connector")
            .field("host", &self.authority().0)
            .field("port", &self.port)
            .finish()
    }
}

/// The connection the connector opened: its TCP socket, read and written
/// through tokio's I/O traits, a task waiting on the socket's wait queue as a
/// guest's poll does.
pub struct Transport {
    sock: Arc<TcpSock>,
    poll_budget: Arc<PollBudget>,
}

impl Transport {
    /// Wait until the connect under way settles, as Linux's
    /// `inet_wait_for_connect` waits for.
    async fn wait_for_connect(&self) -> Result<(), Errno> {
        poll_fn(|cx| self.attempt(cx, || self.sock.take_connect_outcome())).await
    }

    /// `op` once it stops answering `WouldBlock`, the task added to the
    /// socket's wait queue first, so a wake between the two is not lost. Each
    /// answer spends a step of the task's poll budget, as tokio's I/O does,
    /// so a peer that keeps the socket ready never holds one poll.
    fn attempt<T>(
        &self,
        cx: &mut Context<'_>,
        mut op: impl FnMut() -> Result<T, Errno>,
    ) -> Poll<Result<T, Errno>> {
        let step = ready!(self.poll_budget.poll_proceed(cx));
        let done = match op() {
            Err(Errno::WouldBlock) => {
                self.sock.sk_sleep().add(cx.waker());
                match op() {
                    Err(Errno::WouldBlock) => return Poll::Pending,
                    done => done,
                }
            }
            done => done,
        };
        step.made_progress();
        Poll::Ready(done)
    }
}

/// `err` as the I/O error tokio-rustls and hyper carry, holding the failure
/// itself so `Errno::from_io_error` reads back the very one the socket
/// answered.
fn to_io(err: Errno) -> io::Error {
    io::Error::other(CarriedErrno(err))
}

impl AsyncRead for Transport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let read = this.attempt(cx, || this.sock.recvmsg(buf.initialize_unfilled()));
        read.map(|read| read.map(|(n, _)| buf.advance(n)).map_err(to_io))
    }
}

impl AsyncWrite for Transport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.attempt(cx, || this.sock.sendmsg(buf)).map_err(to_io)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.sock.shutdown(Shutdown::Write).map_err(to_io))
    }
}

/// The connection's end releases its socket, which sends what it still
/// holds before its FIN, as a closed socket does.
impl Drop for Transport {
    fn drop(&mut self) {
        self.sock.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A failure the upstream socket answers reaches the HTTP seam through
    // tokio-rustls and hyper, which carry only io::Error.
    #[test]
    fn a_sockets_failure_reads_back_as_itself_through_an_io_error() {
        let failures = [
            Errno::ConnectionReset,
            Errno::TimedOut,
            Errno::OutOfMemory,
            Errno::HostUnreachable,
        ];
        assert_eq!(
            (
                failures
                    .clone()
                    .map(|err| Errno::from_io_error(&to_io(err))),
                HttpError::from_tls(&to_io(Errno::OutOfMemory))
            ),
            (failures, HttpError::Internal),
            "a failure the upstream socket answers must read back as the same errno once it \
             crosses an io::Error, so a failure for want of memory answers internal-error"
        );
    }
}
