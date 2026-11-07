//! The backend that reaches the host network, as Linux's `net_proto_family`
//! makes a socket of one family. A Sandbox holds one, since every entry
//! dials into one namespace; a Rust Host App may hand its own, as gVisor's
//! `hostinet` is one choice of backend.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

/// What dials the host network.
pub trait ProtocolFamily: Send + Sync {
    /// A stream connected to `addr`, given up once `timeout` passes.
    fn connect(&self, addr: SocketAddr, timeout: Option<Duration>) -> io::Result<Box<dyn Stream>>;

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

    /// A datagram socket bound to an ephemeral port and connected to `addr`,
    /// so it sends to and hears from that one peer. A family with no datagrams
    /// answers `Unsupported`.
    fn connect_datagram(&self, _addr: SocketAddr) -> io::Result<Box<dyn Datagram>> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// One datagram socket the backend handed out, connected to one peer.
pub trait Datagram: Send + Sync {
    fn send(&self, buf: &[u8]) -> io::Result<usize>;
    fn recv(&self, buf: &mut [u8]) -> io::Result<usize>;
    /// How long a receive waits before it gives up.
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn local_addr(&self) -> io::Result<SocketAddr>;
}

impl Datagram for UdpSocket {
    fn send(&self, buf: &[u8]) -> io::Result<usize> {
        UdpSocket::send(self, buf)
    }

    fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        UdpSocket::recv(self, buf)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        UdpSocket::set_read_timeout(self, timeout.map(|t| t.max(Duration::from_millis(1))))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        UdpSocket::local_addr(self)
    }
}

/// One connected stream the backend handed out.
pub trait Stream: Read + Write + Send {
    /// A second handle on the same stream, so one worker reads while another
    /// writes.
    fn try_clone(&self) -> io::Result<Box<dyn Stream>>;
    /// Shut one or both directions down, waking a read blocked on it.
    fn shutdown(&self, how: Shutdown) -> io::Result<()>;
    /// How long a read waits before it gives up.
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn local_addr(&self) -> io::Result<SocketAddr>;
    fn peer_addr(&self) -> io::Result<SocketAddr>;
}

/// The host's own TCP, as gVisor's `hostinet` hands a sandbox the host's
/// sockets.
#[derive(Default)]
pub struct HostInet;

impl ProtocolFamily for HostInet {
    fn connect(&self, addr: SocketAddr, timeout: Option<Duration>) -> io::Result<Box<dyn Stream>> {
        let stream = match timeout {
            Some(timeout) => {
                TcpStream::connect_timeout(&addr, timeout.max(Duration::from_millis(1)))?
            }
            None => TcpStream::connect(addr)?,
        };
        stream.set_nodelay(true)?;
        Ok(Box::new(stream))
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
    fn connect_datagram(&self, addr: SocketAddr) -> io::Result<Box<dyn Datagram>> {
        let any: SocketAddr = match addr {
            SocketAddr::V4(_) => ([0, 0, 0, 0], 0).into(),
            SocketAddr::V6(_) => (std::net::Ipv6Addr::UNSPECIFIED, 0).into(),
        };
        let socket = UdpSocket::bind(any)?;
        socket.connect(addr)?;
        Ok(Box::new(socket))
    }

    fn local_addresses(&self) -> Vec<IpAddr> {
        if_addrs::get_if_addrs()
            .map(|interfaces| interfaces.into_iter().map(|i| i.ip()).collect())
            .unwrap_or_default()
    }
}

impl Stream for TcpStream {
    fn try_clone(&self) -> io::Result<Box<dyn Stream>> {
        Ok(Box::new(TcpStream::try_clone(self)?))
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        TcpStream::shutdown(self, how)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, timeout.map(|t| t.max(Duration::from_millis(1))))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        TcpStream::local_addr(self)
    }

    fn peer_addr(&self) -> io::Result<SocketAddr> {
        TcpStream::peer_addr(self)
    }
}
