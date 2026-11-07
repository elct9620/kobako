//! The guest's sockets, as Linux's `net/` makes them: a socket is a file, a
//! Protocol Family backend dials the host, and the guest waits on the
//! socket's wait queue. What may be reached is the Network Grant's to say.

mod family;
mod namespace;
mod tcp;

pub use family::{HostInet, ProtocolFamily, Stream};
pub use namespace::Namespace;
pub use tcp::{sock_alloc_file, TcpSock, SOCKET_BUFFER};
