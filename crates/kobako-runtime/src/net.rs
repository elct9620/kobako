//! The guest's sockets, as Linux's `net/` makes them: a socket is a file, a
//! Protocol Family backend dials the host, and the guest waits on the
//! socket's wait queue. What may be reached is the Network Grant's to say.

mod family;
mod http;
mod lookup;
mod namespace;
mod socket;
mod tcp;
mod tls;
mod workers;

pub use family::{HostInet, ProtocolFamily, Stream};
pub use http::{
    Connector, Exchange, HttpError, Outbound, OutgoingRequest, RequestBody, Response, ResponseHead,
    Scheme, UreqOutbound,
};
pub use lookup::{LookupError, NameLookup};
pub use namespace::Namespace;
pub use socket::{sock_alloc_file, ProtoOps};
pub use tcp::{TcpSock, SOCKET_BUFFER};
pub use tls::platform_tls;
pub use workers::{Workers, PROCESS_WORKERS, SANDBOX_WORKERS};
