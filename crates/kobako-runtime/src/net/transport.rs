//! The TCP link under an HTTP request: the connector that reaches the one
//! authority a request named, through the ruleset and the Protocol Family,
//! and the non-blocking connection the reactor drives for it.

use std::fmt;
use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::connections::Place;
use super::family::admitted_addresses_of;
use super::http::{to_http_error, HttpError};
use super::lookup::NameLookup;
use super::reactor::{Reactor, Ready, Registration};
use super::socket::take_connect_outcome;
use super::ProtocolFamily;
use crate::network::{Protocol, Ruleset};

/// The only dialer the outbound seam has: it reaches the one authority the
/// request named, resolving a name on the host and dialing each address the
/// ruleset lets it lead to, exactly, in turn.
pub struct Connector {
    host: String,
    port: u16,
    ruleset: Arc<Ruleset>,
    family: Arc<dyn ProtocolFamily>,
    /// The request's place, which a lookup of the authority holds too.
    pub(crate) place: Option<Arc<Place>>,
}

impl Connector {
    pub(crate) fn new(
        host: String,
        port: u16,
        ruleset: Arc<Ruleset>,
        family: Arc<dyn ProtocolFamily>,
    ) -> Self {
        Self {
            host,
            port,
            ruleset,
            family,
            place: None,
        }
    }

    /// The host and port the request named.
    pub fn authority(&self) -> (&str, u16) {
        (&self.host, self.port)
    }

    /// A connection to the authority, once one of its addresses answers. The
    /// connector tries each admitted address in turn, as an HTTP client's
    /// connector does, while a guest's TCP socket keeps a loop of its own.
    pub async fn dial(&self) -> Result<Transport, HttpError> {
        let addrs = match self.host.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, self.port)],
            Err(_) => {
                let real = self.resolve().await?;
                admitted_addresses_of(
                    &*self.family,
                    &self.ruleset,
                    &self.host,
                    &real,
                    self.port,
                    Protocol::Http,
                )
            }
        };
        if addrs.is_empty() {
            return Err(HttpError::Denied);
        }
        let mut last = HttpError::ConnectionRefused;
        for addr in addrs {
            let opened = self
                .family
                .connect(addr)
                .and_then(Transport::new)
                .map_err(|err| to_http_error(&err));
            match opened {
                Ok(connection) => match connection.wait_for_connect().await {
                    Ok(()) => return Ok(connection),
                    Err(err) => last = to_http_error(&err),
                },
                Err(err) => last = err,
            }
        }
        Err(last)
    }

    /// The host's answer for the authority's name, from a lookup holding the
    /// request's own place, as a guest's lookup holds one, so its thread
    /// counts against the caps until the resolver returns.
    async fn resolve(&self) -> Result<Vec<IpAddr>, HttpError> {
        let place = self.place.clone().ok_or(HttpError::Internal)?;
        let lookup = NameLookup::start_holding(self.family.clone(), self.host.clone(), place)
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

/// The TCP connection the connector opened, non-blocking, its readiness
/// watched by the reactor, read and written through tokio's I/O traits.
pub struct Transport {
    stream: mio::net::TcpStream,
    registration: Registration,
    waiters: Arc<Waiters>,
}

/// The task waiting to read and the one waiting to write, woken as the
/// reactor sees the connection change.
#[derive(Default)]
struct Waiters {
    read: Mutex<Option<Waker>>,
    write: Mutex<Option<Waker>>,
}

impl Ready for Waiters {
    fn on_event(&self) {
        for waiter in [&self.read, &self.write] {
            if let Some(waker) = waiter.lock().unwrap_or_else(PoisonError::into_inner).take() {
                waker.wake();
            }
        }
    }
}

impl Transport {
    /// The connection `stream` begins, sent without Nagle's delay, as an HTTP
    /// client writes each request whole; a guest's own socket keeps Linux's
    /// default.
    fn new(stream: std::net::TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        let mut stream = mio::net::TcpStream::from_std(stream);
        let waiters = Arc::new(Waiters::default());
        let ready: Arc<dyn Ready> = waiters.clone();
        let registration = Reactor::current()?.register(&mut stream, Arc::downgrade(&ready))?;
        Ok(Self {
            stream,
            registration,
            waiters,
        })
    }

    /// Wait until the connect under way settles, as `SO_ERROR` does and
    /// Linux's `inet_wait_for_connect` waits for.
    async fn wait_for_connect(&self) -> io::Result<()> {
        poll_fn(|cx| {
            self.attempt(cx, false, || {
                take_connect_outcome(&self.stream).unwrap_or(Err(io::ErrorKind::WouldBlock.into()))
            })
        })
        .await
    }

    /// `op` once it stops answering `WouldBlock`, the waker registered first,
    /// so an event between the two is not lost.
    fn attempt<T>(
        &self,
        cx: &mut Context<'_>,
        read: bool,
        mut op: impl FnMut() -> io::Result<T>,
    ) -> Poll<io::Result<T>> {
        loop {
            match op() {
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                done => return Poll::Ready(done),
            }
            let waiter = if read {
                &self.waiters.read
            } else {
                &self.waiters.write
            };
            *waiter.lock().unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
            match op() {
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Poll::Pending,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                done => return Poll::Ready(done),
            }
        }
    }
}

impl AsyncRead for Transport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let read = this.attempt(cx, true, || {
            io::Read::read(&mut &this.stream, buf.initialize_unfilled())
        });
        read.map(|read| read.map(|n| buf.advance(n)))
    }
}

impl AsyncWrite for Transport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.attempt(cx, false, || io::Write::write(&mut &this.stream, buf))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.stream.shutdown(std::net::Shutdown::Write))
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.registration.deregister(&mut self.stream);
    }
}
