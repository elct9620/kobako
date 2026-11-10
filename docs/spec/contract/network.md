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
- `crates/kobako-runtime/src/net/transport.rs`
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

An entry built in code, as Ruby's DSL block builds one. Its endpoints,
credentials, and policies add to those of an entry given beside it.
`Builder::default()` adds to none, as a block given alone does.

```rust
pub struct Builder {}
```

## `Builder::new`

A builder adding to an entry.

```rust
impl Builder {
    pub fn new(entry: Arc<dyn Entry>) -> Self {}
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
    pub allows: bool,
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

## `address_set_of`

The set an address falls in. An IPv6 address carrying an IPv4 one is judged as
the IPv4 inside.

```rust
pub fn address_set_of(ip: IpAddr) -> AddressSet {}
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

## `Ruleset::admitted_addresses_of`

The real addresses a looked-up name resolved to that the Guest may connect to,
in their order. An endpoint naming the name must lead to each, and the host's
own addresses are local.

```rust
impl Ruleset {
    pub fn admitted_addresses_of(&self, name: &str, real: &[IpAddr], port: u16, protocol: Protocol, local: &[IpAddr]) -> Vec<SocketAddr> {}
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

## `Namespace::create_tcp_socket`

Create a TCP socket connected to nothing, as Linux's `sock_create` does. Its
file polls writable and hung up, a read answers `NotConnected`, and a write
`BrokenPipe`. Its buffers count against the Cred's memory, and past it the
socket answers `OutOfMemory`.

```rust
impl Namespace {
    pub fn create_tcp_socket(&self, cred: &Arc<Cred>) -> Result<Arc<TcpSock>, FilesystemError> {}
}
```

## `Namespace::connect`

Start connecting a socket to the address the Guest named, once the socket
hooks and the ruleset pass it. A virtual address dials its name's real
addresses in turn. An unmapped one, and any refusal, answer `PermissionDenied`
and dial nothing.

Once the hooks saw it, an IPv6 address answers `AddressFamilyNotSupported`.
Past a connection limit it answers `WouldBlock`, and a socket already
connecting or connected `AlreadyInProgress` or `AlreadyConnected`.

```rust
impl Namespace {
    pub fn connect(&self, cred: &Cred, sock: &TcpSock, addr: SocketAddr) -> Result<(), FilesystemError> {}
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

## `Namespace::alloc_virtual_address`

Allocate the virtual address a lookup's answer is handed out at. It is never
reused for a second name. `None` answers while the lookup is under way, and
its failure once it failed. `TemporaryFailure` answers once the range is spent,
or once `cred`'s memory cannot hold the entry, which stays to the end.

```rust
impl Namespace {
    pub fn alloc_virtual_address(&self, cred: &Cred, lookup: &NameLookup) -> Option<Result<Ipv4Addr, LookupError>> {}
}
```

## `NameLookup`

One lookup in flight on a thread of its own, then its answer.

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

## `NameLookup::alloc_file`

Allocate the lookup's file, as Linux's `anon_inode_getfile` does. A poll finds
it readable once the lookup answers.

```rust
impl NameLookup {
    pub fn alloc_file(self: &Arc<Self>, cred: &Arc<Cred>) -> File {}
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

The host's own addresses, as Linux's local routing table holds them. A family
that cannot read them answers the failure, and the connect is refused.

```rust
pub trait ProtocolFamily {
    fn local_addresses(&self) -> io::Result<Vec<IpAddr>>;
}
```

## `HostInet`

The host's own TCP, as gVisor's `hostinet` hands a sandbox the host's sockets.

```rust
pub struct HostInet {}
```

## `MAX_SOCKET_BUFFER`

How much a socket holds each way, as `sk_rcvbuf` and `sk_sndbuf` bound one.

```rust
pub const MAX_SOCKET_BUFFER: usize = 64 * 1024;
```

## `MAX_TCP_KEEPIDLE`

The longest idle time a TCP socket takes before its first keepalive probe, in
seconds, as Linux bounds it.

```rust
pub const MAX_TCP_KEEPIDLE: u32 = 32767;
```

## `MAX_TCP_KEEPINTVL`

The longest wait between keepalive probes, in seconds, as Linux bounds it.

```rust
pub const MAX_TCP_KEEPINTVL: u32 = 32767;
```

## `MAX_TCP_KEEPCNT`

The most keepalive probes a TCP socket sends unanswered, as Linux bounds it.

```rust
pub const MAX_TCP_KEEPCNT: u32 = 127;
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

## `TcpSock::finish_connect`

How the connect ended. One still under way answers EAGAIN.

```rust
impl TcpSock {
    pub fn finish_connect(&self) -> Result<(), FilesystemError> {}
}
```

## `TcpSock::send_room`

How many bytes a send would take now.

```rust
impl TcpSock {
    pub fn send_room(&self) -> usize {}
}
```

## `TcpSock::is_drained`

Whether everything sent has left for the host, as Linux's `SIOCOUTQ` reads
zero.

```rust
impl TcpSock {
    pub fn is_drained(&self) -> bool {}
}
```

## `sock_alloc_file`

The socket file of a socket, opened for reading and writing, as Linux's
`sock_alloc_file` makes one. Dropping it releases the socket.

```rust
pub fn sock_alloc_file(sock: Arc<dyn ProtoOps>, cred: &Arc<Cred>) -> File {}
```

## `MemoryCharge`

Memory the host holds for a Guest, reserved against the Invocation's and given
back as it drops. Linux's `sk_forward_alloc` reserves a socket's memory alike.

```rust
pub struct MemoryCharge {}
```

## `MemoryCharge::new`

Reserve some bytes for a Cred, refused past its memory as `OutOfMemory`.

```rust
impl MemoryCharge {
    pub fn new(cred: &Arc<Cred>, bytes: usize) -> Result<Self, FilesystemError> {}
}
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

## `ProtoOps::poll`

What a poll of the socket finds, its wait queue registered on the table, as
a protocol's `->poll` calls `sock_poll_wait`.

```rust
pub trait ProtoOps {
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask;
}
```

## `ProtoOps::shutdown`

Shut the reading side, the writing side, or both down. A socket connected to
nothing marks them and answers `NotConnected`.

```rust
pub trait ProtoOps {
    fn shutdown(&self, how: Shutdown) -> Result<(), FilesystemError>;
}
```

## `ProtoOps::getpeername`

The peer as the Guest named it, once connected. The real peer stays inside the
socket.

```rust
pub trait ProtoOps {
    fn getpeername(&self) -> Option<SocketAddr>;
}
```

## `ProtoOps::getsockname`

The socket's own end as the Guest reads it. It is the Guest's own virtual
address and the port its namespace gave, or the unspecified address before a
connect. The host's own end stays inside the socket, and no lookup hands that
address out.

```rust
pub trait ProtoOps {
    fn getsockname(&self) -> SocketAddr;
}
```

## `ProtoOps::hold_virtual_port`

Hold a port as the socket's own, unless one is held, so a socket connected
again keeps its port.

```rust
pub trait ProtoOps {
    fn hold_virtual_port(&self, alloc: &mut dyn FnMut() -> u16);
}
```

## `ProtoOps::accept`

A connection the socket accepts, as Linux's `proto_ops.accept`. No socket of
the Guest's listens, so a stream refuses as `InvalidInput` and a datagram one
as `Unsupported`.

```rust
pub trait ProtoOps {
    fn accept(&self) -> Result<Arc<dyn ProtoOps>, FilesystemError>;
}
```

## `ProtoOps::getsockopt`

The value an option holds on the socket, never the host's. An option its
protocol has not answers `ProtocolNotAvailable`.

```rust
pub trait ProtoOps {
    fn getsockopt(&self, opt: SockOpt) -> Result<u32, FilesystemError>;
}
```

## `ProtoOps::setsockopt`

Set an option, refusing what Linux refuses. A host socket already dialed takes
it at once.

```rust
pub trait ProtoOps {
    fn setsockopt(&self, opt: SockOpt, value: u32) -> Result<(), FilesystemError>;
}
```

## `ProtoOps::release`

Let go of the host socket once every view is gone.

```rust
pub trait ProtoOps {
    fn release(&self);
}
```

## `SockOpt`

A socket option a protocol may answer. A buffer reads twice what was set.
Sizes are bytes, times are seconds, and a flag is 0 or 1.

```rust
#[non_exhaustive]
pub enum SockOpt {
    RcvBuf,
    SndBuf,
    Ttl,
    KeepAlive,
    KeepIdle,
    KeepIntvl,
    KeepCnt,
}
```

## `Ruleset::admits_request`

Whether an `http` endpoint holding a host at a port allows a method on a path.
With no rules it allows every request.

```rust
impl Ruleset {
    pub fn admits_request(&self, host: &str, port: u16, method: &str, path: &str) -> bool {}
}
```

## `Namespace::send`

An HTTP request in flight, once the socket hooks and an `http` endpoint pass
its authority. A header `to_field_name` or `to_field_value` refuses is refused too.
Any refusal answers `Denied`, and nothing is sent. The body and the
response's buffer count against the Cred's memory, and past it the request
answers `Internal`.

```rust
impl Namespace {
    pub fn send(&self, cred: &Arc<Cred>, request: OutgoingRequest) -> Result<Arc<Exchange>, HttpError> {}
}
```

## `FieldError`

Why a header the Guest names is not one it may set, or why a header list
cannot take it.

```rust
pub enum FieldError {
    InvalidSyntax,
    Forbidden,
    TooLarge,
    OutOfMemory,
}
```

## `MAX_FIELD_SIZE`

The most room one header list the Guest builds may take, as wasmtime's field
limit. Each entry counts its name, its value, and its own place.

```rust
pub const MAX_FIELD_SIZE: usize = 128 * 1024;
```

## `Fields`

A header list the Guest builds or reads. Its room counts against the
Invocation's memory until it drops, so no list grows the host past the Guest's
allotment.

```rust
pub struct Fields {}
```

## `Fields::new`

An empty list, its room charged to a Cred.

```rust
impl Fields {
    pub fn new(cred: Arc<Cred>) -> Arc<Self> {}
}
```

## `Fields::with_entries`

A list holding the given entries, as a response's headers are handed out. Its
room is charged to a Cred, refused past the Invocation's memory as
`OutOfMemory`. `MAX_FIELD_SIZE` bounds only what the Guest appends.

```rust
impl Fields {
    pub fn with_entries(cred: Arc<Cred>, entries: Vec<(String, Vec<u8>)>) -> Result<Arc<Self>, FieldError> {}
}
```

## `Fields::append`

Append a header that `to_field_name` and `to_field_value` take. A list past
`MAX_FIELD_SIZE` refuses it as `TooLarge`, and one past the Invocation's memory
as `OutOfMemory`.

```rust
impl Fields {
    pub fn append(&self, name: &str, value: Vec<u8>) -> Result<(), FieldError> {}
}
```

## `Fields::entries`

Every entry, in the order appended.

```rust
impl Fields {
    pub fn entries(&self) -> Vec<(String, Vec<u8>)> {}
}
```

## `to_field_name`

The header name the Guest may set, lowercase. A name that is no token is
`InvalidSyntax`. One the outbound seam writes itself, as wasmtime's forbidden
list and `content-length`, is `Forbidden`.

```rust
pub fn to_field_name(name: &str) -> Result<String, FieldError> {}
```

## `to_field_value`

The header value the Guest may set. One holding a control byte other than a
tab is `InvalidSyntax`, as an HTTP field value refuses it.

```rust
pub fn to_field_value(value: Vec<u8>) -> Result<Vec<u8>, FieldError> {}
```

## `is_token`

Whether a text is an HTTP token, as a method or a header name must be.

```rust
pub fn is_token(text: &str) -> bool {}
```

## `is_authority`

Whether an authority names a host alone, with no path or user part. A port it
names is a number that fits, as wasmtime asks.

```rust
pub fn is_authority(authority: &str) -> bool {}
```

## `to_origin_form`

A request target in origin form, as one from the Guest is sent. An empty one is
`/`, and a query alone sits on `/`. One that starts otherwise, or that no URI
path holds, is none. A fragment is cut, as the target sent carries none.

```rust
pub fn to_origin_form(path: &str) -> Option<String> {}
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
    fn send_request(&self, request: OutgoingRequest, connector: Connector) -> ResponseFuture;
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

## `Exchange::alloc_response_file`

Allocate the file a poll finds readable once the response's head arrives or
the request fails, as Linux's `anon_inode_getfile` does.

```rust
impl Exchange {
    pub fn alloc_response_file(self: &Arc<Self>, cred: &Arc<Cred>) -> File {}
}
```

## `Exchange::alloc_body_file`

Allocate the file the response's body reads from: zero at its end and
`WouldBlock` while it holds nothing yet. A poll finds it readable once it holds
bytes, ends, or fails.

```rust
impl Exchange {
    pub fn alloc_body_file(self: &Arc<Self>, cred: &Arc<Cred>) -> File {}
}
```

## `Exchange::release`

Let the exchange go, dropping its task with the connection it holds.

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

## `RequestBody::alloc_file`

Allocate the file a Guest writes the body through, as Linux's
`anon_inode_getfile` does. A write appends, refused past the Invocation's
memory as `OutOfMemory`.

```rust
impl RequestBody {
    pub fn alloc_file(self: &Arc<Self>) -> File {}
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

## `new_platform_tls`

A new TLS client configuration trusting the OS's own store, with its own
session store. It is `None` where the platform offers no store. Its verifier is
built once per process.

```rust
pub fn new_platform_tls() -> Option<Arc<ClientConfig>> {}
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

One UDP socket, shared by the socket file and the reactor. The reactor watches
its host socket once it connects to one peer. A read takes one datagram.

```rust
pub struct UdpSock {}
```

## `Namespace::create_udp_socket`

Create a UDP socket connected to nothing, as Linux's `sock_create` does. Its
file polls writable, a read answers `WouldBlock`, and a write
`DestinationAddressRequired`. Its receive queue counts against the Cred's
memory, and past it the socket answers `OutOfMemory`.

```rust
impl Namespace {
    pub fn create_udp_socket(&self, cred: &Arc<Cred>) -> Result<Arc<UdpSock>, FilesystemError> {}
}
```

## `Namespace::connect_datagram`

Connect a UDP socket to the one peer the Guest named. The socket hooks and the
ruleset pass it first, as they pass a TCP connect. A stream naming no peer
answers `PermissionDenied`, and an IPv6 peer `AddressFamilyNotSupported`.

```rust
impl Namespace {
    pub fn connect_datagram(&self, cred: &Cred, sock: &UdpSock, addr: Option<SocketAddr>) -> Result<(), FilesystemError> {}
}
```

## `Namespace::bind`

Bind a socket to an address as a Cred, once the socket hooks pass it, as
Linux's `bind` asks `security_socket_bind`. The Guest only reaches out, so
kobako's own module passes only the unspecified address at port 0. Any other
answers `PermissionDenied`, and an IPv6 address the hooks pass answers
`AddressFamilyNotSupported`.

```rust
impl Namespace {
    pub fn bind(&self, cred: &Cred, addr: SocketAddr) -> Result<(), FilesystemError> {}
}
```
