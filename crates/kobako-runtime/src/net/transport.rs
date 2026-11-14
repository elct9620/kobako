//! The TCP link under an HTTP request: the connector that reaches the one
//! authority a request named, and the connection it opened. The connector
//! dials through a TCP socket of the Invocation's own, as an egress proxy
//! dials through the kernel, so its upstream passes the socket layer's
//! filter, holds a virtual port, and counts its buffers against the
//! Invocation's memory, as a guest's socket does.

use std::fmt;
use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, Shutdown, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::Instant;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::connections::Place;
use super::http::HttpError;
use super::lookup::{NameLookup, Resolver};
use super::namespace::TcpSockets;
use super::socket::{BindHash, Carried, Destination};
use super::{Namespace, ProtoOps, ProtocolFamily, SockOpt, TcpSock};
use crate::cred::Cred;
use crate::network::{Host, Protocol, Ruleset};
use crate::wait::WaitQueueHead;
use crate::Errno;

/// The only dialer the outbound seam has: it reaches the one authority the
/// request named, resolving a name on the host and dialing each real address
/// in turn through a socket the ruleset filters.
pub struct Connector {
    host: Host,
    port: u16,
    ruleset: Arc<Ruleset>,
    family: Arc<dyn ProtocolFamily>,
    resolver: Arc<dyn Resolver>,
    ports: Arc<BindHash>,
    tcp_sockets: Arc<TcpSockets>,
    cred: Arc<Cred>,
    /// The request's place, which a lookup of the authority holds too.
    pub(crate) place: Option<Arc<Place>>,
    /// When a connect opened, which its exchange's wait for the head starts
    /// from.
    pub(crate) connected: Arc<OnceLock<Instant>>,
    /// The queue its exchange's waiters sleep on, woken as the connect opens,
    /// so a wait bound by the connect's timeout takes the head's.
    pub(crate) wait: Option<Arc<WaitQueueHead>>,
}

impl Connector {
    /// The connector for `host` at `port` in `net`, dialing as `cred`.
    pub(crate) fn new(net: &Namespace, cred: &Arc<Cred>, host: Host, port: u16) -> Self {
        Self {
            host,
            port,
            ruleset: net.ruleset().clone(),
            family: net.family().clone(),
            resolver: net.resolver().clone(),
            ports: net.ports_of(Protocol::Http).clone(),
            tcp_sockets: net.tcp_sockets().clone(),
            cred: cred.clone(),
            place: None,
            connected: Arc::default(),
            wait: None,
        }
    }

    /// The host and port the request named.
    pub fn authority(&self) -> (String, u16) {
        (self.host.to_string(), self.port)
    }

    /// A connection to the authority, once one of its addresses answers. The
    /// connector tries each real address in turn, as an HTTP client's
    /// connector does, while a guest's TCP socket keeps a loop of its own; an
    /// address the filter refuses is passed over, and none admitted is
    /// denied.
    pub async fn dial(&self) -> Result<Transport, HttpError> {
        let dests = match &self.host {
            Host::Address(ip) => vec![Destination::Literal(SocketAddr::new(*ip, self.port))],
            Host::Name(name) => self
                .resolve(name)
                .await?
                .into_iter()
                .map(|real| Destination::Named {
                    name: name.clone(),
                    real,
                    port: self.port,
                })
                .collect(),
        };
        let mut last = HttpError::Denied;
        for dest in &dests {
            let sock = TcpSock::create_kern(&self.family, &self.tcp_sockets, &self.cred)
                .map_err(|err| HttpError::from_errno(&err))?;
            // Set before the dial, so the host socket takes it as it is
            // made, as an HTTP client writes each request whole; a guest's
            // own socket keeps Linux's default.
            sock.setsockopt(SockOpt::NoDelay, 1)
                .map_err(|err| HttpError::from_errno(&err))?;
            match sock.connect_upstream(&self.ports, &self.ruleset, dest) {
                Err(Errno::PermissionDenied) => continue,
                Err(err) => {
                    last = HttpError::from_errno(&err);
                    continue;
                }
                Ok(()) => {}
            }
            let connection = Transport { sock };
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

    /// The host's answer for `name`, the authority's, from a lookup holding
    /// the request's own place, as a guest's lookup holds one, so its thread
    /// counts against the caps until the resolver returns.
    async fn resolve(&self, name: &str) -> Result<Vec<IpAddr>, HttpError> {
        let place = self.place.clone().ok_or(HttpError::Internal)?;
        let lookup = NameLookup::start_holding(self.resolver.clone(), name.to_string(), place)
            .map_err(|_| HttpError::ConnectionLimitReached)?;
        poll_fn(|cx| lookup.poll_answer(cx))
            .await
            .map_err(|_| HttpError::DestinationNotFound)
    }
}

impl fmt::Debug for Connector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connector")
            .field("host", &self.host)
            .field("port", &self.port)
            .finish()
    }
}

/// The connection the connector opened: its TCP socket, read and written
/// through tokio's I/O traits, a task waiting on the socket's wait queue as a
/// guest's poll does.
pub struct Transport {
    sock: Arc<TcpSock>,
}

impl Transport {
    /// Wait until the connect under way settles, as Linux's
    /// `inet_wait_for_connect` waits for.
    async fn wait_for_connect(&self) -> Result<(), Errno> {
        poll_fn(|cx| self.attempt(cx, || self.sock.connect_outcome())).await
    }

    /// `op` once it stops answering `WouldBlock`, the task added to the
    /// socket's wait queue first, so a wake between the two is not lost.
    fn attempt<T>(
        &self,
        cx: &mut Context<'_>,
        mut op: impl FnMut() -> Result<T, Errno>,
    ) -> Poll<Result<T, Errno>> {
        match op() {
            Err(Errno::WouldBlock) => {}
            done => return Poll::Ready(done),
        }
        self.sock.sk_sleep().add(cx.waker());
        match op() {
            Err(Errno::WouldBlock) => Poll::Pending,
            done => Poll::Ready(done),
        }
    }
}

/// `err` as the I/O error tokio-rustls and hyper carry, holding the failure
/// itself so `to_error` reads back the very one the socket answered.
fn to_io(err: Errno) -> io::Error {
    io::Error::other(Carried(err))
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
    use super::super::socket::to_error;
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
                failures.clone().map(|err| to_error(&to_io(err))),
                HttpError::from_io(&to_io(Errno::OutOfMemory))
            ),
            (failures, HttpError::Internal),
            "a failure the upstream socket answers must read back as the same errno once it \
             crosses an io::Error, so memory the socket could not reserve answers internal-error"
        );
    }
}
