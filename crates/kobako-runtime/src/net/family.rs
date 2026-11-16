//! The backend that reaches the host network, as Linux's `net_proto_family`
//! makes a socket of one family. Every entry of an Invocation dials into one
//! namespace through one family, which `Backends` names, as gVisor's
//! `hostinet` is one choice of backend for a sandbox. Each socket it hands
//! out is non-blocking, so the reactor waits on it and no call blocks a
//! thread.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};

use super::lookup::{Resolver, Resolving};
use crate::network::{Protocol, Ruleset};

/// What dials the host network.
pub trait ProtocolFamily: Send + Sync {
    /// A non-blocking stream to `addr`, whose connect may still be under way,
    /// as a non-blocking `connect` answers `EINPROGRESS`.
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream>;

    /// The host's own addresses, as Linux's `local` routing table holds them,
    /// read at each connect. A family with none answers an empty list; one
    /// that cannot read them, or does not say, answers a failure, and the
    /// connect is refused rather than judged as if the host had none.
    fn local_addresses(&self) -> io::Result<Vec<IpAddr>> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// A non-blocking datagram socket bound to an ephemeral port and connected
    /// to `addr`, so it sends to and hears from that one peer. A family with
    /// no datagrams answers `Unsupported`.
    fn connect_datagram(&self, _addr: SocketAddr) -> io::Result<UdpSocket> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// Where a datagram socket this family handed out, bound at `local`,
    /// sends once the guest points it at `addr`, keeping its port, as
    /// `__ip4_datagram_connect` routes a socket connected again; `None` when
    /// the socket cannot, as one in another family than where it sends, and a
    /// fresh one is made through `connect_datagram`. The host's own family
    /// sends to `addr` itself; one that sends elsewhere from
    /// `connect_datagram` answers alike here.
    fn route_datagram(
        &self,
        local: SocketAddr,
        addr: SocketAddr,
    ) -> io::Result<Option<SocketAddr>> {
        Ok(is_same_family(local, addr).then_some(addr))
    }
}

/// Whether `local` and `peer` are both IPv4 or both IPv6, as a socket bound
/// at one may connect to the other.
pub(crate) fn is_same_family(local: SocketAddr, peer: SocketAddr) -> bool {
    local.is_ipv4() == peer.is_ipv4()
}

/// The host's own network: its TCP and UDP sockets, as gVisor's `hostinet`
/// hands a sandbox the host's sockets, and the host's resolver.
#[derive(Default)]
pub struct HostInet;

impl ProtocolFamily for HostInet {
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream> {
        Ok(mio::net::TcpStream::connect(addr)?.into())
    }

    /// The host's own UDP, bound to an ephemeral port of `addr`'s family.
    fn connect_datagram(&self, addr: SocketAddr) -> io::Result<UdpSocket> {
        let socket = UdpSocket::bind(super::resolv::any_address_for(addr))?;
        socket.connect(addr)?;
        socket.set_nonblocking(true)?;
        Ok(socket)
    }

    fn local_addresses(&self) -> io::Result<Vec<IpAddr>> {
        super::resolv::local_addresses()
    }
}

impl Resolver for HostInet {
    /// The host's resolver, asked for `name` with its trailing dot, so no
    /// search domain lengthens it.
    fn resolve(&self, name: &str) -> Resolving {
        super::resolv::resolve(&format!("{name}."))
    }
}

/// The addresses `real`, resolved from `name`, the ruleset admits to `port`
/// over `protocol`, judged against the host's own addresses as `family` reads
/// them. A host whose addresses cannot be read admits none, so an unreadable
/// interface list refuses a connect rather than judging as if the host had
/// none. Every path that dials a name passes through here.
pub(crate) fn admitted_addresses_of(
    family: &dyn ProtocolFamily,
    ruleset: &Ruleset,
    name: &str,
    real: &[IpAddr],
    port: u16,
    protocol: Protocol,
) -> Vec<SocketAddr> {
    match family.local_addresses() {
        Ok(local) => ruleset.admitted_addresses_of(name, real, port, protocol, &local),
        Err(_) => Vec::new(),
    }
}

/// Protocol Families the net tests share.
#[cfg(test)]
pub(crate) mod testing {
    use std::future::Future;
    use std::io;
    use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::task::{Context, Poll};

    use super::{ProtocolFamily, Resolver};
    use crate::net::lookup::LookupError;

    /// A family refusing every dial, TCP or UDP, counting how often it is
    /// asked, beside a resolver that resolves nothing.
    #[derive(Default)]
    pub(crate) struct CountsDials(AtomicUsize);

    impl CountsDials {
        /// How many dials it refused.
        pub(crate) fn dials(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    impl ProtocolFamily for CountsDials {
        fn connect(&self, _addr: SocketAddr) -> io::Result<TcpStream> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(io::ErrorKind::ConnectionRefused.into())
        }

        fn connect_datagram(&self, _addr: SocketAddr) -> io::Result<UdpSocket> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(io::ErrorKind::ConnectionRefused.into())
        }
    }

    impl Resolver for CountsDials {
        fn resolve(&self, _name: &str) -> crate::net::Resolving {
            Box::pin(std::future::ready(Err(LookupError::Fail)))
        }
    }

    /// A host refusing every dial, whose resolver says each query started,
    /// then never answers, counting the queries dropped.
    pub(crate) struct StallsLookups {
        starts: Mutex<mpsc::Sender<()>>,
        cancelled: Arc<AtomicUsize>,
    }

    impl StallsLookups {
        pub(crate) fn new(starts: mpsc::Sender<()>) -> Self {
            Self {
                starts: Mutex::new(starts),
                cancelled: Arc::default(),
            }
        }

        /// How many queries were dropped unanswered.
        pub(crate) fn cancelled(&self) -> usize {
            self.cancelled.load(Ordering::SeqCst)
        }
    }

    impl ProtocolFamily for StallsLookups {
        fn connect(&self, _addr: SocketAddr) -> io::Result<TcpStream> {
            Err(io::ErrorKind::ConnectionRefused.into())
        }
    }

    impl Resolver for StallsLookups {
        fn resolve(&self, _name: &str) -> crate::net::Resolving {
            let _ = self.starts.lock().unwrap().send(());
            Box::pin(Stalled(self.cancelled.clone()))
        }
    }

    /// One query that never answers, counted as it drops.
    struct Stalled(Arc<AtomicUsize>);

    impl Future for Stalled {
        type Output = Result<Vec<IpAddr>, LookupError>;

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Pending
        }
    }

    impl Drop for Stalled {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}
