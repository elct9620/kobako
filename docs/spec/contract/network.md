# Network interface

What a Host App writes against to give the Guest the network. Each named entry
is one Grant, and its endpoints are data the runtime judges on any thread.

## Includes

- `crates/kobako-runtime/src/network.rs`
- `crates/kobako-runtime/src/network/builder.rs`
- `crates/kobako-runtime/src/network/ruleset.rs`
- `crates/kobako-runtime/src/net/family.rs`
- `crates/kobako-runtime/src/net/http.rs`
- `crates/kobako-runtime/src/net/lookup.rs`
- `crates/kobako-runtime/src/net/tls.rs`
- `crates/kobako-runtime/src/net/udp.rs`
- `crates/kobako-runtime/src/net/namespace.rs`
- `crates/kobako-runtime/src/net/socket.rs`
- `crates/kobako-runtime/src/net/tcp.rs`
- `crates/kobako-runtime/src/net/connections.rs`

## `Network`

One named entry of the network, handed over for an Invocation.

```rust
pub struct Network {}
```

## `Network::new`

The entry under a name, refused for an empty name.

```rust
impl Network {
    pub fn new(name: impl Into<String>, entry: Arc<dyn Entry>) -> Result<Self, EntryError> {}
}
```

## `Network::name`

The name a Context replaces the entry by.

```rust
impl Network {
    pub fn name(&self) -> &str {}
}
```

## `Network::endpoints`

What the entry lets the Guest reach, as it answers for this Invocation.

```rust
impl Network {
    pub fn endpoints(&self) -> Vec<Endpoint> {}
}
```

## `Network::policies`

What the entry's policies judge, as it answers for this Invocation.

```rust
impl Network {
    pub fn policies(&self) -> Policies {}
}
```

## `Entry::endpoints`

What the Guest may reach through the entry; one playing no such role reaches
nothing.

```rust
pub trait Entry {
    fn endpoints(&self) -> Vec<Endpoint>;
}
```

## `Entry::policies`

The hooks the entry's policies answer; one it leaves out has no opinion.

```rust
pub trait Entry {
    fn policies(&self) -> Policies;
}
```

## `Entry::credentials`

The credentials swapped into a request bound to the entry's own endpoints.

```rust
pub trait Entry {
    fn credentials(&self) -> Vec<Credential>;
}
```

## `Policies`

An entry's policies, one optional hook per decision point. A missing hook has
no opinion, and a policy only narrows.

```rust
pub struct Policies {
    pub resolve: Option<Arc<dyn for<'a> Policy<Name<'a>>>>,
    pub connect: Option<Arc<dyn for<'a> Policy<Connection<'a>>>>,
    pub request: Option<Arc<dyn for<'a> Policy<Request<'a>>>>,
}
```

## `Policies::with_request`

These policies with one judging each HTTP request.

```rust
impl Policies {
    pub fn with_request(self, policy: Arc<dyn for<'a> Policy<Request<'a>>>) -> Self {}
}
```

## `Request`

One HTTP request the Guest sends: the authority, method, scheme, path with no
query, and header names. It never holds a header's value.

```rust
pub struct Request<'a> {
    pub authority: (&'a str, u16),
    pub method: &'a str,
    pub scheme: &'a str,
    pub path: &'a str,
    pub header_names: &'a [&'a str],
}
```

## `Request::new`

The request to an authority by a method, scheme, path, and header names.

```rust
impl<'a> Request<'a> {
    pub fn new(authority: (&'a str, u16), method: &'a str, scheme: &'a str, path: &'a str, header_names: &'a [&'a str]) -> Self {}
}
```

## `Builder::request_policy`

Judge each HTTP request with one more policy; any refusal wins.

```rust
impl Builder {
    pub fn request_policy(&mut self, policy: impl for<'a> Policy<Request<'a>> + 'static) -> &mut Self {}
}
```

## `Policies::with_resolve`

These policies with one judging each lookup.

```rust
impl Policies {
    pub fn with_resolve(self, policy: Arc<dyn for<'a> Policy<Name<'a>>>) -> Self {}
}
```

## `Name`

One name the Guest asks to look up, lowercase and with no trailing dot.

```rust
pub struct Name<'a> {
    pub name: &'a str,
}
```

## `Name::new`

The name to look up.

```rust
impl<'a> Name<'a> {
    pub fn new(name: &'a str) -> Self {}
}
```

## `Policies::with_connect`

These policies with one judging each connect.

```rust
impl Policies {
    pub fn with_connect(self, policy: Arc<dyn for<'a> Policy<Connection<'a>>>) -> Self {}
}
```

## `Builder`

An entry built in code, as Ruby's DSL block builds one. Its endpoints and
policies add to those of an entry given beside it.

```rust
pub struct Builder {}
```

## `Builder::on`

A builder adding to an entry.

```rust
impl Builder {
    pub fn on(entry: Arc<dyn Entry>) -> Self {}
}
```

## `Builder::endpoint`

Let the Guest reach one more endpoint.

```rust
impl Builder {
    pub fn endpoint(&mut self, endpoint: Endpoint) -> &mut Self {}
}
```

## `Builder::resolve_policy`

Judge each lookup with one more policy; any refusal wins.

```rust
impl Builder {
    pub fn resolve_policy(&mut self, policy: impl for<'a> Policy<Name<'a>> + 'static) -> &mut Self {}
}
```

## `Builder::connect_policy`

Judge each connect with one more policy; any refusal wins.

```rust
impl Builder {
    pub fn connect_policy(&mut self, policy: impl for<'a> Policy<Connection<'a>> + 'static) -> &mut Self {}
}
```

## `Connection`

One connection the Guest asks for: the host it named, its port, and its
protocol. The host is never a real address a name resolved to.

```rust
pub struct Connection<'a> {
    pub host: &'a str,
    pub port: u16,
    pub protocol: Protocol,
}
```

## `Connection::new`

The connection to a host, a port, and a protocol.

```rust
impl<'a> Connection<'a> {
    pub fn new(host: &'a str, port: u16, protocol: Protocol) -> Self {}
}
```

## `Endpoint`

What the Guest may reach: a host, a port, a protocol, and where the host's name
may lead.

```rust
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub addresses: Vec<Addresses>,
    pub rules: Vec<HttpRule>,
}
```

## `Endpoint::with_rules`

This endpoint judging each request by the rules given.

```rust
impl Endpoint {
    pub fn with_rules(self, rules: Vec<HttpRule>) -> Self {}
}
```

## `HttpRule`

One rule an `http` endpoint judges a request by: a method or any, and a path
pattern. `*` stands for one segment, and `**` for any number.

```rust
pub struct HttpRule {
    pub allow: bool,
    pub method: Option<String>,
    pub path: String,
}
```

## `HttpRule::allow`

A rule allowing a method, or any, on the paths a pattern matches.

```rust
impl HttpRule {
    pub fn allow(method: Option<&str>, path: impl Into<String>) -> Self {}
}
```

## `HttpRule::deny`

A rule denying a method, or any, on the paths a pattern matches.

```rust
impl HttpRule {
    pub fn deny(method: Option<&str>, path: impl Into<String>) -> Self {}
}
```

## `Endpoint::new`

An endpoint speaking HTTP to public addresses, refused for an empty host or
port zero.

```rust
impl Endpoint {
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self, EntryError> {}
}
```

## `Endpoint::with_protocol`

The endpoint speaking another protocol.

```rust
impl Endpoint {
    pub fn with_protocol(self, protocol: Protocol) -> Self {}
}
```

## `Endpoint::with_addresses`

The endpoint leading to other addresses.

```rust
impl Endpoint {
    pub fn with_addresses(self, addresses: Vec<Addresses>) -> Self {}
}
```

## `Protocol`

What an endpoint speaks: HTTP, TCP, or UDP.

```rust
pub enum Protocol {}
```

## `Addresses`

Where an endpoint's name may lead: an address set, or a CIDR.

```rust
pub enum Addresses {}
```

## `EntryError`

An entry or an endpoint no connection could follow.

```rust
pub struct EntryError {}
```

## `AddressSet`

Where an address falls, by IANA's special-purpose registries.

```rust
pub enum AddressSet {}
```

## `address_set`

The set an address falls in. An IPv6 address carrying an IPv4 one is judged as
the IPv4 inside.

```rust
pub fn address_set(ip: IpAddr) -> AddressSet {}
```

## `Ruleset`

What one Invocation's entries let the Guest reach, compiled from every
endpoint.

```rust
pub struct Ruleset {}
```

## `Ruleset::new`

The ruleset of the endpoints, refused for a host naming no address, prefix, or
name.

```rust
impl Ruleset {
    pub fn new(endpoints: impl IntoIterator<Item = Endpoint>) -> Result<Self, EntryError> {}
}
```

## `Ruleset::admit_literal`

Whether the Guest may connect to a literal address it named. The fixed sets
refuse whatever an endpoint says.

```rust
impl Ruleset {
    pub fn admit_literal(&self, addr: SocketAddr, protocol: Protocol) -> Result<(), FilesystemError> {}
}
```

## `Ruleset::admits_name`

Whether any endpoint's host names a name, so a lookup of it may run.

```rust
impl Ruleset {
    pub fn admits_name(&self, name: &str) -> bool {}
}
```

## `Ruleset::admit_resolved`

Whether the Guest may connect to a real address the name it looked up resolved
to. An endpoint naming the name must lead there, and the host's own addresses
are local.

```rust
impl Ruleset {
    pub fn admit_resolved(&self, name: &str, addr: SocketAddr, protocol: Protocol, local: &[IpAddr]) -> Result<(), FilesystemError> {}
}
```

## `Namespace`

The network one Invocation's entries make together: the ruleset, the connect
policies, and the Protocol Family that dials.

```rust
pub struct Namespace {}
```

## `Namespace::new`

The namespace of every entry given, dialing through a Protocol Family and sending
HTTP through an outbound seam. Each connection counts against the Sandbox's
open connections while open. The Invocation makes at most its connection limit
in all. It is refused for an endpoint the ruleset cannot compile.

```rust
impl Namespace {
    pub fn new<'a>(networks: impl IntoIterator<Item = &'a Network>, family: Arc<dyn ProtocolFamily>, outbound: Arc<dyn Outbound>, open_connections: Arc<OpenConnections>, connection_limit: Option<usize>) -> Result<Self, EntryError> {}
}
```

## `Namespace::connect`

A TCP socket to the address the Guest named, once the socket hooks and the
ruleset pass it. A virtual address dials its name's real address. An
unmapped one, and any refusal, answer `PermissionDenied`, and nothing is
dialed.

```rust
impl Namespace {
    pub fn connect(&self, cred: &Cred, addr: SocketAddr) -> Result<Arc<TcpSock>, FilesystemError> {}
}
```

## `Namespace::resolve`

A lookup of a name, normalized first. A name a policy refuses or no endpoint
names answers as NXDOMAIN before any query.

```rust
impl Namespace {
    pub fn resolve(&self, name: &str) -> Result<Arc<NameLookup>, LookupError> {}
}
```

## `Namespace::virtual_address`

The virtual address a lookup's answer is handed out at. It is never reused for
a second name, and `None` answers once the range is spent.

```rust
impl Namespace {
    pub fn virtual_address(&self, name: &str, real: Vec<IpAddr>) -> Option<Ipv4Addr> {}
}
```

## `NameLookup`

One lookup in flight on a worker, then its answer.

```rust
pub struct NameLookup {}
```

## `NameLookup::name`

The name looked up, normalized.

```rust
impl NameLookup {
    pub fn name(&self) -> &str {}
}
```

## `NameLookup::answer`

The real addresses the name resolved to, or why it has none; `None` while the
lookup is under way.

```rust
impl NameLookup {
    pub fn answer(&self) -> Option<Result<Vec<IpAddr>, LookupError>> {}
}
```

## `NameLookup::poll`

Whether the lookup has answered, registering on a poll table as a file's poll
does.

```rust
impl NameLookup {
    pub fn poll(&self, table: Option<&mut PollTable>) -> bool {}
}
```

## `LookupError`

Why a lookup has no address. A refused name answers as an unresolvable one.

```rust
pub enum LookupError {
    NameUnresolvable,
    PermanentFailure,
    TemporaryFailure,
}
```

## `ProtocolFamily`

The backend that reaches the host network, as Linux's `net_proto_family`. A
Rust Host App may hand its own. Each socket it hands out is non-blocking, so
the reactor waits on it.

```rust
pub trait ProtocolFamily: Send + Sync {}
```

## `ProtocolFamily::connect`

A non-blocking stream to the address, whose connect may still be under way.

```rust
pub trait ProtocolFamily {
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream>;
}
```

## `ProtocolFamily::resolve`

The addresses a fully qualified name resolves to. A family that resolves
nothing answers `Unsupported`.

```rust
pub trait ProtocolFamily {
    fn resolve(&self, _name: &str) -> io::Result<Vec<IpAddr>>;
}
```

## `ProtocolFamily::local_addresses`

The host's own addresses, as Linux's local routing table holds them.

```rust
pub trait ProtocolFamily {
    fn local_addresses(&self) -> Vec<IpAddr>;
}
```

## `HostInet`

The host's own TCP, as gVisor's `hostinet` hands a sandbox the host's sockets.

```rust
pub struct HostInet {}
```

## `SOCKET_BUFFER`

How much a socket holds each way, as `sk_rcvbuf` and `sk_sndbuf` bound one.

```rust
pub const SOCKET_BUFFER: usize = 64 * 1024;
```

## `OpenConnections`

How many connections one owner holds open, and its most, as a cgroup's
`pids.max` bounds its tasks. A socket, a request, and a lookup in flight each
hold one. A forked child starts from none.

```rust
pub struct OpenConnections {}
```

## `OpenConnections::new`

An owner holding at most this many at once; `None` lifts the limit.

```rust
impl OpenConnections {
    pub fn new(limit: Option<usize>) -> Arc<Self> {}
}
```

## `OpenConnections::set_limit`

Move the limit, as writing `pids.max` does. One held past a lowered limit stays
open, and the next is refused.

```rust
impl OpenConnections {
    pub fn set_limit(&self, limit: Option<usize>) {}
}
```

## `process_connections`

Every connection the process's Sandboxes hold open together, 512 at most until
a Rust Host App moves it.

```rust
pub fn process_connections() -> &'static Arc<OpenConnections> {}
```

## `TcpSock`

One TCP socket, shared by its socket file and the reactor watching its host
socket.

```rust
pub struct TcpSock {}
```

## `TcpSock::connect`

A socket connecting to the first address that answers, in turn, without
blocking. It answers at once. The socket counts against the Sandbox's and the
process's caps until released, and a full count answers `WouldBlock`.

```rust
impl TcpSock {
    pub fn connect(family: Arc<dyn ProtocolFamily>, addrs: Vec<SocketAddr>, open_connections: &Arc<OpenConnections>) -> Result<Arc<Self>, FilesystemError> {}
}
```

## `TcpSock::finish_connect`

How the connect ended. One still under way answers EAGAIN.

```rust
impl TcpSock {
    pub fn finish_connect(&self) -> Result<(), FilesystemError> {}
}
```

## `TcpSock::recv`

What the receive buffer holds. A drained one answers EAGAIN until the peer ends.

```rust
impl TcpSock {
    pub fn recv(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {}
}
```

## `TcpSock::send`

Queue what fits for the writer. A full buffer answers EAGAIN.

```rust
impl TcpSock {
    pub fn send(&self, buf: &[u8]) -> Result<usize, FilesystemError> {}
}
```

## `TcpSock::send_room`

How many bytes a send would take now.

```rust
impl TcpSock {
    pub fn send_room(&self) -> usize {}
}
```

## `TcpSock::shutdown`

Shut the writing side down once what is queued is sent, or stop reading.

```rust
impl TcpSock {
    pub fn shutdown(&self, how: Shutdown) {}
}
```

## `TcpSock::poll_mask`

What a call would find ready, as `tcp_poll` answers.

```rust
impl TcpSock {
    pub fn poll_mask(&self) -> PollMask {}
}
```

## `TcpSock::wait_queue`

What a guest's poll registers on.

```rust
impl TcpSock {
    pub fn wait_queue(&self) -> &Arc<WaitQueueHead> {}
}
```

## `TcpSock::local_addr`

The address the socket's own end stands at, once connected.

```rust
impl TcpSock {
    pub fn local_addr(&self) -> Option<SocketAddr> {}
}
```

## `TcpSock::peer_addr`

The address the socket's peer stands at, once connected.

```rust
impl TcpSock {
    pub fn peer_addr(&self) -> Option<SocketAddr> {}
}
```

## `sock_alloc_file`

The socket file of a socket, opened for reading and writing, as Linux's
`sock_alloc_file` makes one. Dropping it releases the socket.

```rust
pub fn sock_alloc_file(sock: Arc<dyn ProtoOps>, cred: &Arc<Cred>) -> File {}
```

## `ProtoOps`

What one protocol answers for its socket file, as Linux's `struct proto_ops`.

```rust
pub trait ProtoOps: Send + Sync {}
```

## `ProtoOps::recvmsg`

Read what the socket holds; `WouldBlock` while it holds nothing yet.

```rust
pub trait ProtoOps {
    fn recvmsg(&self, buf: &mut [u8]) -> Result<usize, FilesystemError>;
}
```

## `ProtoOps::sendmsg`

Write what the socket takes now; `WouldBlock` while it takes nothing.

```rust
pub trait ProtoOps {
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, FilesystemError>;
}
```

## `ProtoOps::poll_mask`

What a poll of the socket finds.

```rust
pub trait ProtoOps {
    fn poll_mask(&self) -> PollMask;
}
```

## `ProtoOps::wait_queue`

What a Guest's poll of the socket waits on.

```rust
pub trait ProtoOps {
    fn wait_queue(&self) -> &Arc<WaitQueueHead>;
}
```

## `ProtoOps::release`

Let go of the host socket once every view is gone.

```rust
pub trait ProtoOps {
    fn release(&self);
}
```

## `Ruleset::admit_request`

Whether an `http` endpoint holding a host at a port allows a method on a path.
With no rules it allows every request.

```rust
impl Ruleset {
    pub fn admit_request(&self, host: &str, port: u16, method: &str, path: &str) -> bool {}
}
```

## `Namespace::send`

An HTTP request in flight, once the socket hooks and an `http` endpoint pass
its authority. A header `field_name` or `field_value` refuses is refused too.
Any refusal answers `Denied`, and nothing is sent.

```rust
impl Namespace {
    pub fn send(&self, cred: &Cred, request: OutgoingRequest) -> Result<Arc<Exchange>, HttpError> {}
}
```

## `FieldError`

Why a header the Guest names is not one it may set.

```rust
pub enum FieldError {
    InvalidSyntax,
    Forbidden,
}
```

## `field_name`

The header name the Guest may set, lowercase. A name that is no token is
`InvalidSyntax`. One the outbound seam writes itself, as wasmtime's forbidden
list and `content-length`, is `Forbidden`.

```rust
pub fn field_name(name: &str) -> Result<String, FieldError> {}
```

## `field_value`

The header value the Guest may set. One holding CR, LF, or NUL is
`InvalidSyntax`.

```rust
pub fn field_value(value: Vec<u8>) -> Result<Vec<u8>, FieldError> {}
```

## `is_token`

Whether a text is an HTTP token, as a method or a header name must be.

```rust
pub fn is_token(text: &str) -> bool {}
```

## `is_authority`

Whether an authority names a host alone, with no path or user part.

```rust
pub fn is_authority(authority: &str) -> bool {}
```

## `is_origin_form`

Whether a request target is in origin form, as one from the Guest must be.

```rust
pub fn is_origin_form(path: &str) -> bool {}
```

## `Scheme`

Whether a request travels in clear or over TLS.

```rust
pub enum Scheme {
    Http,
    Https,
}
```

## `OutgoingRequest`

One request the Guest sends: its method, scheme, authority, path with its query,
headers, and body.

```rust
pub struct OutgoingRequest {
    pub method: String,
    pub scheme: Scheme,
    pub authority: String,
    pub path_with_query: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}
```

## `OutgoingRequest::new`

A request for a method, a scheme, and an authority, at the root path.

```rust
impl OutgoingRequest {
    pub fn new(method: impl Into<String>, scheme: Scheme, authority: impl Into<String>) -> Self {}
}
```

## `HttpError`

Why a request has no response, as `wasi:http` names each a client meets. None
carries host text.

```rust
pub enum HttpError {
    Denied,
    DestinationNotFound,
    ConnectionRefused,
    ConnectionTimeout,
    ConnectionTerminated,
    ProtocolError,
    ConnectionLimitReached,
    TlsCertificate,
    TlsProtocol,
    Internal,
}
```

## `ResponseHead`

A response's status and headers.

```rust
pub struct ResponseHead {
    pub status: u16,
    pub headers: Vec<(String, Vec<u8>)>,
}
```

## `Response`

What the outbound seam answers: the head, and the body to stream.

```rust
pub struct Response {
    pub head: ResponseHead,
    pub body: ResponseBody,
}
```

## `Transport`

The TCP connection the connector opened, non-blocking, read and written
through tokio's I/O traits.

```rust
pub struct Transport {}
```

## `HyperOutbound`

The default outbound seam: hyper's HTTP/1 client with no redirect, proxy, or
decompression. Kobako's connector opens each connection, and TLS ends on the
host. Each seam holds its own TLS configuration.

```rust
pub struct HyperOutbound {}
```

## `HyperOutbound::new`

A seam trusting the OS's own store.

```rust
impl HyperOutbound {
    pub fn new() -> Self {}
}
```

## `Outbound`

The replaceable HTTP sender. It dials only through the connector it is handed,
and its send runs on the reactor, so it never blocks.

```rust
pub trait Outbound: Send + Sync {}
```

## `Outbound::send_request`

Send a request through the connector.

```rust
pub trait Outbound {
    fn send_request(&self, request: OutgoingRequest, connector: Connector) -> Sending;
}
```

## `Connector`

The outbound seam's only dialer. It reaches the one authority the request
named, each address the ruleset admits, exactly, in turn.

```rust
pub struct Connector {}
```

## `Connector::authority`

The host and port the request named.

```rust
impl Connector {
    pub fn authority(&self) -> (&str, u16) {}
}
```

## `Connector::dial`

A connection to the authority, once one of its addresses answers.

```rust
impl Connector {
    pub async fn dial(&self) -> Result<Transport, HttpError> {}
}
```

## `Exchange`

One request in flight, then its response's head and body.

```rust
pub struct Exchange {}
```

## `Exchange::response`

The response's head, or why it has none; `None` while the request is under way.

```rust
impl Exchange {
    pub fn response(&self) -> Option<Result<ResponseHead, HttpError>> {}
}
```

## `Exchange::read`

Read what the body holds: zero at its end and `WouldBlock` while it holds
nothing yet.

```rust
impl Exchange {
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {}
}
```

## `Exchange::poll`

What a poll finds, the head or the body ready, registering on a poll table.

```rust
impl Exchange {
    pub fn poll(&self, table: Option<&mut PollTable>) -> PollMask {}
}
```

## `Exchange::release`

Let the exchange go, so a worker blocked on a full buffer ends.

```rust
impl Exchange {
    pub fn release(&self) {}
}
```

## `RequestBody`

The body a Guest writes for a request. Its bytes count against the
Invocation's memory, so a Guest cannot grow the host past its allotment.

```rust
pub struct RequestBody {}
```

## `RequestBody::new`

An empty body, its bytes charged to a Cred.

```rust
impl RequestBody {
    pub fn new(cred: Arc<Cred>) -> Arc<Self> {}
}
```

## `RequestBody::write`

Append bytes, refused past the Invocation's memory as `OutOfMemory`.

```rust
impl RequestBody {
    pub fn write(&self, buf: &[u8]) -> Result<usize, FilesystemError> {}
}
```

## `RequestBody::take`

Take what was written, given back to the Invocation's memory as the request
leaves.

```rust
impl RequestBody {
    pub fn take(&self) -> Vec<u8> {}
}
```

## `platform_tls`

A TLS client configuration trusting the OS's own store, with its own session
store. It is `None` where the platform offers no store. Its verifier is built
once per process.

```rust
pub fn platform_tls() -> Option<Arc<ClientConfig>> {}
```

## `Network::credentials`

The credentials the entry swaps into requests bound to its own endpoints.

```rust
impl Network {
    pub fn credentials(&self) -> Vec<Credential> {}
}
```

## `Secret`

A value the Host App hands no Guest. Its `Debug` prints a mask, and its bytes
are zeroed as it drops.

```rust
pub struct Secret {}
```

## `Secret::new`

A secret holding these bytes.

```rust
impl Secret {
    pub fn new(value: impl Into<Vec<u8>>) -> Self {}
}
```

## `Secret::expose`

The bytes, for the one header they are swapped into.

```rust
impl Secret {
    pub fn expose(&self) -> &[u8] {}
}
```

## `Placement`

Where a credential's value goes: the `authorization` header as a bearer token
or a basic pair, or one named header.

```rust
pub enum Placement {
    Bearer,
    Basic,
    Header(String),
}
```

## `Placement::header`

The header the value is swapped into.

```rust
impl Placement {
    pub fn header(&self) -> &str {}
}
```

## `CredentialSource`

What resolves a credential's value as a bound request leaves. A closure fits.

```rust
pub trait CredentialSource: Send + Sync {}
```

## `CredentialSource::value`

The value for a request, or `None` to send nothing.

```rust
pub trait CredentialSource {
    fn value(&self, request: &Request<'_>) -> Option<Secret>;
}
```

## `Credential`

A placement and a placeholder paired with the source of the value that
replaces the placeholder there.

```rust
pub struct Credential {
    pub placement: Placement,
    pub placeholder: String,
    pub source: Arc<dyn CredentialSource>,
}
```

## `Credential::new`

The credential of a placement, a placeholder, and a source.

```rust
impl Credential {
    pub fn new(placement: Placement, placeholder: impl Into<String>, source: Arc<dyn CredentialSource>) -> Self {}
}
```

## `Builder::credential`

Swap one more credential into requests bound to this entry's endpoints.

```rust
impl Builder {
    pub fn credential(&mut self, credential: Credential) -> &mut Self {}
}
```

## `Ruleset::binds`

Whether a credential of these endpoints' entry is bound to a request. An `http`
endpoint must name its exact host and allow it, on an unambiguous path.

```rust
impl Ruleset {
    pub fn binds(&self, host: &str, port: u16, method: &str, path: &str) -> bool {}
}
```

## `ProtocolFamily::connect_datagram`

A non-blocking datagram socket bound to an ephemeral port and connected to one
peer. A family with no datagrams answers `Unsupported`.

```rust
pub trait ProtocolFamily {
    fn connect_datagram(&self, _addr: SocketAddr) -> io::Result<UdpSocket>;
}
```

## `MAX_DATAGRAM`

The most one IPv4 datagram carries.

```rust
pub const MAX_DATAGRAM: usize = 65_507;
```

## `UdpSock`

One UDP socket connected to one peer, shared by the socket file and the
reactor watching its host socket. A read takes one datagram.

```rust
pub struct UdpSock {}
```

## `UdpSock::connect`

A socket connected to a peer, its host socket watched by the reactor. It counts
against the Sandbox's and the process's caps until released.

```rust
impl UdpSock {
    pub fn connect(family: Arc<dyn ProtocolFamily>, addr: SocketAddr, open_connections: &Arc<OpenConnections>) -> Result<Arc<Self>, FilesystemError> {}
}
```

## `UdpSock::local_addr`

The host's own end, once the socket opened.

```rust
impl UdpSock {
    pub fn local_addr(&self) -> Option<SocketAddr> {}
}
```

## `Namespace::connect_datagram`

A UDP socket streaming with the one peer the Guest named. The socket hooks and
the ruleset pass it first, as they pass a TCP connect. A stream naming no peer
answers `PermissionDenied`.

```rust
impl Namespace {
    pub fn connect_datagram(&self, cred: &Cred, addr: Option<SocketAddr>) -> Result<Arc<UdpSock>, FilesystemError> {}
}
```

## `Namespace::bind`

Whether a socket may bind an address. Only the unspecified address at port 0
passes, since the Guest only reaches out; any other answers
`PermissionDenied`.

```rust
impl Namespace {
    pub fn bind(&self, addr: SocketAddr) -> Result<(), FilesystemError> {}
}
```
