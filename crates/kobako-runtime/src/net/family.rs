//! The backend that reaches the host network, as Linux's `net_proto_family`
//! makes a socket of one family. A Sandbox holds one, since every entry
//! dials into one namespace; a Rust Host App may hand its own, as gVisor's
//! `hostinet` is one choice of backend. Each socket it hands out is
//! non-blocking, so the reactor waits on it and no call blocks a thread.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};

/// What dials the host network.
pub trait ProtocolFamily: Send + Sync {
    /// A non-blocking stream to `addr`, whose connect may still be under way,
    /// as a non-blocking `connect` answers `EINPROGRESS`.
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream>;

    /// The addresses `name`, a fully qualified name, resolves to. A family
    /// that resolves nothing answers `Unsupported`.
    fn resolve(&self, _name: &str) -> io::Result<Vec<IpAddr>> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// The host's own addresses, as Linux's `local` routing table holds them,
    /// read at each connect. A family with none answers an empty list.
    fn local_addresses(&self) -> Vec<IpAddr> {
        Vec::new()
    }

    /// A non-blocking datagram socket bound to an ephemeral port and connected
    /// to `addr`, so it sends to and hears from that one peer. A family with
    /// no datagrams answers `Unsupported`.
    fn connect_datagram(&self, _addr: SocketAddr) -> io::Result<UdpSocket> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// The host's own TCP, as gVisor's `hostinet` hands a sandbox the host's
/// sockets.
#[derive(Default)]
pub struct HostInet;

impl ProtocolFamily for HostInet {
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream> {
        let stream = mio::net::TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(stream.into())
    }

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

    fn local_addresses(&self) -> Vec<IpAddr> {
        if_addrs::get_if_addrs()
            .map(|interfaces| interfaces.into_iter().map(|i| i.ip()).collect())
            .unwrap_or_default()
    }
}
