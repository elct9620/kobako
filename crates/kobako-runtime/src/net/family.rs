//! The backend that reaches the host network, as Linux's `net_proto_family`
//! makes a socket of one family. The Wasm Driver hands each Invocation one,
//! since every entry dials into one namespace; a Wasm Driver of its own may
//! hand another, as gVisor's `hostinet` is one choice of backend. Each socket it hands out is
//! non-blocking, so the reactor waits on it and no call blocks a thread.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};

use super::lookup::Resolver;
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
        let any: SocketAddr = match addr {
            SocketAddr::V4(_) => ([0, 0, 0, 0], 0).into(),
            SocketAddr::V6(_) => (std::net::Ipv6Addr::UNSPECIFIED, 0).into(),
        };
        let socket = UdpSocket::bind(any)?;
        socket.connect(addr)?;
        socket.set_nonblocking(true)?;
        Ok(socket)
    }

    fn local_addresses(&self) -> io::Result<Vec<IpAddr>> {
        if_addrs::get_if_addrs().map(|interfaces| interfaces.into_iter().map(|i| i.ip()).collect())
    }
}

impl Resolver for HostInet {
    /// The host's resolver, asked for `name` with its trailing dot, so no
    /// search domain lengthens it.
    fn resolve(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        let mut addresses = Vec::new();
        for addr in (format!("{name}."), 0).to_socket_addrs()? {
            if !addresses.contains(&addr.ip()) {
                addresses.push(addr.ip());
            }
        }
        Ok(addresses)
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
    use std::io;
    use std::net::{SocketAddr, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{ProtocolFamily, Resolver};

    /// A family refusing every dial, counting how often it is asked, beside a
    /// resolver that resolves nothing.
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
    }

    impl Resolver for CountsDials {
        fn resolve(&self, _name: &str) -> io::Result<Vec<std::net::IpAddr>> {
            Err(io::ErrorKind::Unsupported.into())
        }
    }
}
