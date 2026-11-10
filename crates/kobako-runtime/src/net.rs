//! The guest's sockets, as Linux's `net/` makes them: a socket is a file, a
//! Protocol Family backend dials the host, and the guest waits on the
//! socket's wait queue. What may be reached is the Network Grant's to say.

mod connections;
mod credential;
mod egress;
mod family;
mod http;
mod lookup;
mod namespace;
mod reactor;
mod socket;
mod tcp;
mod tls;
mod transport;
mod udp;

pub use connections::{process_connections, OpenConnections};
pub use family::{HostInet, ProtocolFamily};
pub use http::{
    is_authority, is_scheme, is_token, to_field_name, to_field_value, to_origin_form, Exchange,
    FieldError, Fields, HttpError, HyperOutbound, Outbound, OutgoingRequest, RequestBody, Response,
    ResponseBody, ResponseFuture, ResponseHead, Scheme, MAX_FIELD_SIZE,
};
pub use lookup::{LookupError, NameLookup};
pub use namespace::Namespace;
pub use socket::{sock_alloc_file, sock_from_file, MemoryCharge, ProtoOps, SockOpt, SockType};
pub use tcp::{TcpSock, MAX_SOCKET_BUFFER, MAX_TCP_KEEPCNT, MAX_TCP_KEEPIDLE, MAX_TCP_KEEPINTVL};
pub use tls::new_platform_tls;
pub use transport::{Connector, Transport};
pub use udp::{UdpSock, MAX_DATAGRAM};
