//! The backend that reaches the host network, as Linux's `net_proto_family`
//! makes a socket of one family. A Sandbox holds one, since every entry
//! dials into one namespace; a Rust Host App may hand its own, as gVisor's
//! `hostinet` is one choice of backend.

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

/// What dials the host network.
pub trait ProtocolFamily: Send + Sync {
    /// A stream connected to `addr`, given up once `timeout` passes.
    fn connect(&self, addr: SocketAddr, timeout: Option<Duration>) -> io::Result<Box<dyn Stream>>;
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
