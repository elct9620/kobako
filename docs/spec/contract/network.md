# Network interface

What a Host App writes against to give the Guest the network. Each named entry
is one Grant, and its endpoints are data the runtime judges on any thread.

## Includes

- `crates/kobako-runtime/src/network.rs`
- `crates/kobako-runtime/src/network/builder.rs`
- `crates/kobako-runtime/src/network/ruleset.rs`
- `crates/kobako-runtime/src/net/family.rs`
- `crates/kobako-runtime/src/net/lookup.rs`
- `crates/kobako-runtime/src/net/namespace.rs`
- `crates/kobako-runtime/src/net/tcp.rs`
- `crates/kobako-runtime/src/net/workers.rs`

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

## `Policies`

An entry's policies, one optional hook per decision point. A missing hook has
no opinion, and a policy only narrows.

```rust
pub struct Policies {
    pub resolve: Option<Arc<dyn for<'a> Policy<Name<'a>>>>,
    pub connect: Option<Arc<dyn for<'a> Policy<Connection<'a>>>>,
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

The namespace of every entry given, dialing through a Protocol Family. It is
refused for an endpoint the ruleset cannot compile.

```rust
impl Namespace {
    pub fn new<'a>(networks: impl IntoIterator<Item = &'a Network>, family: Arc<dyn ProtocolFamily>, workers: Arc<Workers>) -> Result<Self, EntryError> {}
}
```

## `Namespace::connect`

A TCP socket to the address the Guest named, once the socket hooks and the
ruleset pass it. A virtual address dials its name's real address, and an
unmapped one answers `NetworkUnreachable`. Any refusal answers
`PermissionDenied`, and nothing is dialed.

```rust
impl Namespace {
    pub fn connect(&self, cred: &Cred, addr: SocketAddr, deadline: Option<Instant>) -> Result<Arc<TcpSock>, FilesystemError> {}
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
Rust Host App may hand its own.

```rust
pub trait ProtocolFamily: Send + Sync {}
```

## `ProtocolFamily::connect`

A stream connected to the address, given up once the timeout passes.

```rust
pub trait ProtocolFamily {
    fn connect(&self, addr: SocketAddr, timeout: Option<Duration>) -> io::Result<Box<dyn Stream>>;
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

## `Stream`

One connected stream a backend handed out.

```rust
pub trait Stream: Read + Write + Send {}
```

## `Stream::try_clone`

A second handle on the same stream, so one worker reads while another writes.

```rust
pub trait Stream {
    fn try_clone(&self) -> io::Result<Box<dyn Stream>>;
}
```

## `Stream::shutdown`

Shut one or both directions down, waking a read blocked on it.

```rust
pub trait Stream {
    fn shutdown(&self, how: Shutdown) -> io::Result<()>;
}
```

## `Stream::set_read_timeout`

How long a read waits before it gives up.

```rust
pub trait Stream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
}
```

## `Stream::local_addr`

The address the stream's own end stands at.

```rust
pub trait Stream {
    fn local_addr(&self) -> io::Result<SocketAddr>;
}
```

## `Stream::peer_addr`

The address the stream's peer stands at.

```rust
pub trait Stream {
    fn peer_addr(&self) -> io::Result<SocketAddr>;
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

## `Workers`

How many worker threads one owner runs, and the most it may, as a cgroup's
`pids.max` bounds its tasks. A forked child starts from none.

```rust
pub struct Workers {}
```

## `Workers::new`

An owner running at most this many workers.

```rust
impl Workers {
    pub fn new(limit: usize) -> Arc<Self> {}
}
```

## `SANDBOX_WORKERS`

The most worker threads one Sandbox runs at once, two per open socket.

```rust
pub const SANDBOX_WORKERS: usize = 64;
```

## `PROCESS_WORKERS`

The most worker threads the process runs at once, across every Sandbox.

```rust
pub const PROCESS_WORKERS: usize = 1024;
```

## `TcpSock`

One TCP socket, shared by its socket file and the workers doing its host I/O.

```rust
pub struct TcpSock {}
```

## `TcpSock::connect`

A socket connecting on a worker to the first address that answers, in turn,
given up at the deadline. It answers at once. Its two workers count against the
Sandbox's and the process's, and a full count answers `WouldBlock`.

```rust
impl TcpSock {
    pub fn connect(family: Arc<dyn ProtocolFamily>, addrs: Vec<SocketAddr>, deadline: Option<Instant>, workers: &Arc<Workers>) -> Result<Arc<Self>, FilesystemError> {}
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
pub fn sock_alloc_file(sock: &Arc<TcpSock>, cred: &Arc<Cred>) -> File {}
```
