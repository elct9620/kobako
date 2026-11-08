//! The guest's sockets, as Linux's `net/` makes them: a socket is a file, a
//! Protocol Family backend dials the host, and the guest waits on the
//! socket's wait queue. What may be reached is the Network Grant's to say.

mod connections;
mod family;
mod http;
mod lookup;
mod namespace;
mod reactor;
mod socket;
mod tcp;
mod tls;
mod udp;

pub use connections::{process_connections, OpenConnections};
pub use family::{HostInet, ProtocolFamily};
pub use http::{
    field_name, field_value, is_authority, is_origin_form, is_token, Connector, Exchange,
    FieldError, HttpError, HyperOutbound, Outbound, OutgoingRequest, RequestBody, Response,
    ResponseBody, ResponseHead, Scheme, Sending, Transport,
};
pub use lookup::{LookupError, NameLookup};
pub use namespace::Namespace;
pub use socket::{sock_alloc_file, ProtoOps};
pub use tcp::{TcpSock, MAX_SOCKET_BUFFER};
pub use tls::platform_tls;
pub use udp::{UdpSock, MAX_DATAGRAM};
