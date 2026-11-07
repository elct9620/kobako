# Network interface

What a Host App writes against to give the Guest the network. Each named entry
is one Grant, and its endpoints are data the runtime judges on any thread.

## Includes

- `crates/kobako-runtime/src/network.rs`
- `crates/kobako-runtime/src/net/family.rs`
- `crates/kobako-runtime/src/net/tcp.rs`

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

## `TcpSock`

One TCP socket, shared by its socket file and the workers doing its host I/O.

```rust
pub struct TcpSock {}
```

## `TcpSock::connect`

A socket connecting on a worker, given up at the deadline. It answers at once.

```rust
impl TcpSock {
    pub fn connect(family: Arc<dyn ProtocolFamily>, addr: SocketAddr, deadline: Option<Instant>) -> Arc<Self> {}
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
