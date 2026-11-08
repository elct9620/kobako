# Network

How a Host App hands the guest the network. Each named entry is one Grant, seals with the Sandbox, and is replaceable for one invocation.

```
sandbox.network(name, entry) ─ sealed at the first invocation
ctx.network(name, ...)       ─ replaces it for that invocation only
each invocation ─ Grants ─> runtime ─ wasi:sockets, wasi:http ─> guest
```

### Why these scenarios

An entry is a registration, as a mount is, so it is refused once the Sandbox is sealed. A name means one entry, as a path means one mount. A Context replaces an entry by its name but never adds one. An endpoint is data, so its defaults are what an endpoint written without them reaches.

```
endpoint "api.example.com", port: 443
  protocol  -> :http
  addresses -> [:public]
```

## Includes

- `test/e2e/test_network.rb`
- `test/e2e/test_network_tcp.rb`
- `test/e2e/test_network_names.rb`
- `test/e2e/test_network_http.rb`
- `test/e2e/test_network_http_policy.rb`
- `test/e2e/test_network_https.rb`
- `test/e2e/test_network_udp.rb`
- `test/parity/test_network.rb`
- `test/unit/catalog/test_network.rb`
- `crates/kobako/tests/integration/network.rs`
- `crates/kobako-runtime/src/network.rs`
- `crates/kobako-runtime/src/network/builder.rs`
- `crates/kobako-runtime/src/network/ruleset.rs`
- `crates/kobako-runtime/src/net/http.rs`
- `crates/kobako-runtime/src/net/namespace.rs`
- `crates/kobako-runtime/src/net/connections.rs`
- `crates/kobako-runtime/src/net/udp.rs`
- `crates/kobako-wasmtime/src/preview2/host.rs`
- `crates/kobako-wasmtime/src/preview2/http.rs`

## `NW-001` An entry after the first invocation is refused

| Attribute | Value |
| --- | --- |
| relates | `MT-002` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox that has run an invocation |
| When | a network entry is given to it |
| Then | the entry is refused |

## `NW-002` A name given twice is refused

| Attribute | Value |
| --- | --- |
| relates | `MT-004` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a network entry named `github` |
| When | another entry named `github` is given to it |
| Then | the second entry is refused |

## `NW-003` A name and a mount path never meet

| Attribute | Value |
| --- | --- |
| relates | `NW-002`, `MT-004` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Filesystem mounted at `/data` |
| When | a network entry named `/data` is given to it |
| Then | both stand, each judged only against its own kind |

## `NW-004` A runtime presenting no network refuses the entry

| Attribute | Value |
| --- | --- |
| relates | `MT-006` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox over a runtime that supports no network |
| When | a network entry is given to it |
| Then | the entry is refused as a setup failure |

## `NW-005` A Context cannot replace a name the Sandbox never declared

| Attribute | Value |
| --- | --- |
| relates | `MT-010` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with no network entry named `other` |
| When | an invocation's Context replaces the entry named `other` |
| Then | the override is refused before the guest runs |

## `NW-006` A Context replaces an entry for its invocation only

| Attribute | Value |
| --- | --- |
| relates | `MT-009` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a network entry named `api` |
| When | an invocation's Context replaces the entry named `api` |
| Then | that invocation receives the replacement, and the next receives the Sandbox's own |

## `NW-007` An endpoint reaches HTTP and public addresses unless written otherwise

| Step | Statement |
| --- | --- |
| Given | an endpoint written with only a host and a port |
| When | its protocol and its addresses are read |
| Then | the protocol is HTTP and the addresses are the public set |

## `NW-008` An empty name or a malformed endpoint is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an entry is given an empty name, or an endpoint whose host is empty or whose port is zero |
| Then | the entry is refused |

## `NW-009` Without the network, creating a socket answers not-supported

| Attribute | Value |
| --- | --- |
| refines | `RT-067` |

| Step | Statement |
| --- | --- |
| Given | a guest with no network entry |
| When | it creates a TCP socket through `wasi:sockets` |
| Then | the call answers `not-supported` at once, as WASI's host without a network does |

## `NW-010` The guest's network is one handle it may drop

| Step | Statement |
| --- | --- |
| Given | a guest |
| When | it asks `instance-network` for its network and drops the handle |
| Then | the drop succeeds, and dropping it again traps |

## `NW-011` Without the network, a TCPSocket raises NotImplementedError

| Attribute | Value |
| --- | --- |
| refines | `NW-009` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with no network entry |
| When | the script opens a `TCPSocket` |
| Then | it raises `NotImplementedError`, as a file opened with no Mount does |

## `NW-012` Each address falls in one address set

| Step | Statement |
| --- | --- |
| Given | an address the guest may reach |
| When | its address set is read |
| Then | IANA's registries class it public, private, local, metadata, or reserved; an IPv6 address carrying an IPv4 one is that IPv4 |

## `NW-013` A literal address passes only an endpoint naming it

| Attribute | Value |
| --- | --- |
| relates | `NW-007` |

| Step | Statement |
| --- | --- |
| Given | entries whose endpoints name an address and a prefix |
| When | the guest connects to a literal address |
| Then | only an address an endpoint's host holds, at its port and protocol, passes; any other is refused |

## `NW-014` Metadata and reserved addresses never open

| Attribute | Value |
| --- | --- |
| refines | `NW-013` |

| Step | Statement |
| --- | --- |
| Given | an endpoint naming a metadata or a reserved address |
| When | the guest connects to that address |
| Then | the connection is refused, since kobako's fixed sets sit in no entry |

## `NW-015` An HTTP endpoint admits no raw TCP

| Attribute | Value |
| --- | --- |
| refines | `NW-013` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming an address |
| When | the guest opens raw TCP to that address and port |
| Then | the connection is refused, so its rules cannot be framed around |

## `NW-016` A TCPSocket to an endpoint's address carries bytes both ways

| Attribute | Value |
| --- | --- |
| relates | `NW-013` |

| Step | Statement |
| --- | --- |
| Given | a TCP endpoint naming a loopback address and port with a server listening |
| When | the script opens a `TCPSocket` there, writes, closes its write side, and reads |
| Then | the server receives the bytes, and the script reads the server's answer up to its close |

## `NW-017` A TCPSocket no endpoint admits raises Errno::EACCES

| Attribute | Value |
| --- | --- |
| refines | `NW-013` |

| Step | Statement |
| --- | --- |
| Given | a TCP endpoint naming a different port than the one the script reaches |
| When | the script opens a `TCPSocket` to that address and port |
| Then | it raises `Errno::EACCES`, as a denied `connect` does on Linux |

## `NW-018` Any entry's connect policy refuses what the endpoints allow

| Attribute | Value |
| --- | --- |
| refines | `NW-013` |

| Step | Statement |
| --- | --- |
| Given | an endpoint admitting an address, and entries whose connect policies judge it |
| When | the guest connects to that address |
| Then | each policy sees the host the guest named, its port, and its protocol. One refusal answers `EACCES`, and nothing is dialed |

## `NW-019` A Wasm Driver's security module refuses before an entry's policy

| Attribute | Value |
| --- | --- |
| refines | `NW-018` |

| Step | Statement |
| --- | --- |
| Given | a Wasm Driver built with a security module refusing every connect |
| When | the guest connects to an address an endpoint admits |
| Then | the connect answers `EACCES` before any entry's policy is asked, as Linux stacks its modules |

## `NW-020` A builder adds its endpoints and policies to the entry it is given

| Attribute | Value |
| --- | --- |
| relates | `NW-018` |

| Step | Statement |
| --- | --- |
| Given | an entry, and a builder adding an endpoint and a connect policy to it |
| When | the built entry is read |
| Then | its endpoints are the entry's then the builder's, and either's connect policy refusing refuses |

## `NW-021` A Ruby connect policy refuses a TCPSocket the endpoints allow

| Attribute | Value |
| --- | --- |
| refines | `NW-018` |

| Step | Statement |
| --- | --- |
| Given | an entry's `connect?`, or a `policy(:connect)` block, beside an endpoint admitting an address |
| When | the script opens a `TCPSocket` there |
| Then | the policy is handed a `Kobako::Network::Connection`. A falsy answer raises `Errno::EACCES` in the script |

## `NW-022` A full open connection limit answers EAGAIN

| Attribute | Value |
| --- | --- |
| relates | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose sockets, requests, and lookups fill its open connection limit, or the process's |
| When | the guest connects once more |
| Then | the connect answers `EAGAIN`, never a Trap, and dials nothing. A socket released gives its place back, and a forked child starts from none |

## `NW-023` A frozen clock beside a socket elapses in host time

| Attribute | Value |
| --- | --- |
| refines | `RT-338` |

| Step | Statement |
| --- | --- |
| Given | a guest at the strongest posture with the network |
| When | it polls a silent socket beside a clock fifty milliseconds out |
| Then | the clock answers once fifty milliseconds of host time pass, since a host-side producer takes host time |

## `NW-024` A name no endpoint names answers as NXDOMAIN unasked

| Attribute | Value |
| --- | --- |
| relates | `NW-013` |

| Step | Statement |
| --- | --- |
| Given | endpoints naming an exact name and a wildcard, and a `resolve?` policy refusing one name |
| When | the guest looks up a name no endpoint names, the refused name, or the wildcard's own apex |
| Then | each answers `name-unresolvable`, as NXDOMAIN does, and no query reaches the resolver |

## `NW-025` A name connects through its virtual address to where its endpoint leads

| Attribute | Value |
| --- | --- |
| relates | `NW-024` |

| Step | Statement |
| --- | --- |
| Given | a name its lookup handed a virtual address, from the bottom of `198.18.0.0/15` |
| When | the guest connects to that virtual address |
| Then | the real address the endpoint's `addresses` lead to is dialed. An endpoint leading only to public addresses refuses a local one |

## `NW-026` A virtual address no lookup handed out is refused

| Attribute | Value |
| --- | --- |
| refines | `NW-025` |

| Step | Statement |
| --- | --- |
| Given | a guest that looked up no name |
| When | it connects to an address in the virtual range |
| Then | the connect answers `EACCES`, as any refused destination does, and nothing is dialed |

## `NW-027` A connect dials what the lookup answered

| Attribute | Value |
| --- | --- |
| refines | `NW-025` |

| Step | Statement |
| --- | --- |
| Given | a name whose lookup answered one address, then the resolver changes its answer |
| When | the guest connects to the name's virtual address |
| Then | the address the lookup answered is dialed. The name is never resolved again, so rebinding has no second answer to use |

## `NW-028` The host's own address is local

| Attribute | Value |
| --- | --- |
| refines | `NW-025` |

| Step | Statement |
| --- | --- |
| Given | a name resolving to one of the host's own addresses, whatever registry classes it |
| When | the guest connects through an endpoint leading only to public addresses |
| Then | the connect is refused, since the host's addresses fall in `local`, as Linux's `local` table holds them |

## `NW-029` A wildcard endpoint leads only to public addresses or a CIDR

| Attribute | Value |
| --- | --- |
| relates | `NW-024` |

| Step | Statement |
| --- | --- |
| Given | a wildcard endpoint whose `addresses` name `private` or `local` |
| When | the ruleset compiles |
| Then | it is refused, since a guest-controlled name beneath the wildcard could reach them; `public` and a CIDR stand |

## `NW-030` Without the network, a lookup answers permanent-resolver-failure

| Attribute | Value |
| --- | --- |
| refines | `NW-009` |

| Step | Statement |
| --- | --- |
| Given | a guest with no network entry |
| When | it looks up a name through `wasi:sockets` |
| Then | the call answers `permanent-resolver-failure` at once, as a host with no resolver does |

## `NW-031` A socket connected through a virtual address reads it back as its peer

| Attribute | Value |
| --- | --- |
| refines | `NW-025` |

| Step | Statement |
| --- | --- |
| Given | a name an endpoint names, looked up through `ip-name-lookup` |
| When | the guest connects through the one virtual address the lookup hands out |
| Then | the lookup ends after that address, and `remote-address` answers the virtual address, as cgroup `getpeername4` maps a real peer back |

## `NW-032` Each address a name may lead to is dialed in turn

| Attribute | Value |
| --- | --- |
| refines | `NW-025` |

| Step | Statement |
| --- | --- |
| Given | a name answering `::1` first, then `127.0.0.1`, through an endpoint leading to `local` |
| When | the guest connects and `::1` refuses |
| Then | `127.0.0.1` is dialed next, so a name answering both loopbacks still connects |

## `NW-033` A TCPSocket reaches a name and shows its virtual address

| Attribute | Value |
| --- | --- |
| refines | `NW-031` |

| Step | Statement |
| --- | --- |
| Given | a TCP endpoint naming `localhost` and leading to `local`, with a server listening |
| When | the script opens a `TCPSocket` to `localhost`, writes, and reads |
| Then | the bytes round-trip, and `peeraddr` shows the virtual address, where CRuby shows the real one |

## `NW-034` A TCPSocket to a name no endpoint names raises Socket::ResolutionError

| Attribute | Value |
| --- | --- |
| refines | `NW-024` |

| Step | Statement |
| --- | --- |
| Given | entries whose endpoints name no such name |
| When | the script opens a `TCPSocket` to it |
| Then | it raises `Socket::ResolutionError`, a `SocketError`, with Linux's `getaddrinfo` words and its `EAI_NONAME` code, as CRuby does |

## `NW-035` A DNS endpoint needs an exact name

| Attribute | Value |
| --- | --- |
| relates | `NW-029` |

| Step | Statement |
| --- | --- |
| Given | an endpoint on port 53 or 853 whose host is a wildcard, an address, or a prefix |
| When | the ruleset compiles |
| Then | it is refused, since such an endpoint opens a channel past kobako's resolver; an exact name stands |

## `NW-036` A Ruby resolve policy refuses a name an endpoint names

| Attribute | Value |
| --- | --- |
| refines | `NW-024` |

| Step | Statement |
| --- | --- |
| Given | an entry's `resolve?`, or a `policy(:resolve)` block, beside an endpoint naming a name |
| When | the script opens a `TCPSocket` to that name, in any case or with a trailing dot |
| Then | the policy is handed the name lowercase with no trailing dot. A falsy answer raises `Socket::ResolutionError` |

## `NW-037` A request to an HTTP endpoint reads back the response

| Attribute | Value |
| --- | --- |
| relates | `NW-025` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming a name and leading to `local`, with a server listening |
| When | the guest sends a request to that authority, in any case |
| Then | the request line reaches the server, and the guest reads back the status, the headers, and the body |

## `NW-038` A request no HTTP endpoint names is denied unsent

| Attribute | Value |
| --- | --- |
| refines | `NW-037` |

| Step | Statement |
| --- | --- |
| Given | entries whose only endpoint is a `tcp` one |
| When | the guest sends a request to its authority, to a name no endpoint names, or to a virtual address |
| Then | each answers `Denied`, and nothing reaches the server |

## `NW-039` Without the network, a request answers HTTP-request-denied

| Attribute | Value |
| --- | --- |
| refines | `NW-009` |

| Step | Statement |
| --- | --- |
| Given | a guest with no network entry |
| When | it hands a request to `outgoing-handler` |
| Then | the call answers `HTTP-request-denied` at once, doing nothing first |

## `NW-040` A request built through wasi:http reads back the response

| Attribute | Value |
| --- | --- |
| refines | `NW-037` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming a server's address |
| When | the guest builds a GET through `wasi:http`, hands it to `outgoing-handler`, and reads the response |
| Then | the server reads the request line. The guest reads the status, a header by any case, and the body to its end |

## `NW-041` A forbidden header or a response's header cannot be set

| Attribute | Value |
| --- | --- |
| relates | `NW-040` |

| Step | Statement |
| --- | --- |
| Given | a guest building headers through `wasi:http` |
| When | it sets `host`, `content-length`, a name that is no token, or a header of a response |
| Then | the first two answer `forbidden`, the name `invalid-syntax`, and the response's header `immutable`; the seam frames the body itself |

## `NW-042` A script's Net::HTTP reaches a server an HTTP endpoint names

| Attribute | Value |
| --- | --- |
| refines | `NW-040` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming a loopback server's address and port |
| When | the script sends a GET with a header, or a POST with a body, through `Net::HTTP` |
| Then | the server reads the path, the header, and the body. The script reads the code, message, body, and headers as CRuby's `Net::HTTPResponse` gives them |

## `NW-043` A Net::HTTP request no endpoint admits raises what CRuby would

| Attribute | Value |
| --- | --- |
| refines | `NW-042` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose endpoints name no such authority, or one with no network entry |
| When | the script sends a request through `Net::HTTP` |
| Then | it raises `Errno::EACCES` wrapped as CRuby wraps a failed open, or `NotImplementedError` with no network |

## `NW-044` An HTTP endpoint's rules admit only what they allow

| Attribute | Value |
| --- | --- |
| refines | `NW-037` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint allowing GET on `/repos/**` and denying `/repos/*/secrets`, and one with no rules |
| When | the guest sends requests by several methods and paths |
| Then | only an allowed method and path that no rule denies passes. A path servers may split differently is refused, and an endpoint with no rules passes any request |

## `NW-045` A request policy sees the request and refuses it unsent

| Attribute | Value |
| --- | --- |
| refines | `NW-037` |

| Step | Statement |
| --- | --- |
| Given | an entry's request policy beside an `http` endpoint |
| When | the guest sends a request with a header |
| Then | the policy sees the authority, method, scheme, path with no query, and header names, never a value. Its refusal answers `Denied`, and nothing is sent |

## `NW-046` A Ruby endpoint's rules refuse a request they do not allow

| Attribute | Value |
| --- | --- |
| refines | `NW-044` |

| Step | Statement |
| --- | --- |
| Given | a `#network` block whose endpoint allows only GET on `/repos/**` |
| When | the script sends a GET and a POST there through `Net::HTTP` |
| Then | the GET reaches the server, and the POST raises `Errno::EACCES` |

## `NW-047` A Ruby request policy is handed the request

| Attribute | Value |
| --- | --- |
| refines | `NW-045` |

| Step | Statement |
| --- | --- |
| Given | a `policy(:request)` block beside an `http` endpoint |
| When | the script sends a POST with a query and a header through `Net::HTTP` |
| Then | the block is handed a `Kobako::Network::Request` with the method, scheme, path, and header names. A falsy answer raises `Errno::EACCES` |

## `NW-048` An HTTPS request opens TLS before any HTTP

| Attribute | Value |
| --- | --- |
| refines | `NW-037` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming a server that speaks only clear text |
| When | the guest sends a request there over `https` |
| Then | the server reads a TLS handshake record first. The request fails as a TLS protocol error, never sent in clear |

## `NW-049` A script's TLS to a server speaking clear raises OpenSSL::SSL::SSLError

| Attribute | Value |
| --- | --- |
| refines | `NW-048` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming a server that answers in clear |
| When | the script sends a request through `Net::HTTP` with `use_ssl: true` |
| Then | it raises `OpenSSL::SSL::SSLError`, an `OpenSSL::OpenSSLError`, as CRuby's does |

## `NW-050` A credential binds only to HTTPS, its exact host, and an allowed path

| Attribute | Value |
| --- | --- |
| refines | `NW-044` |

| Step | Statement |
| --- | --- |
| Given | an entry binding a bearer credential to an exact host whose rules allow one path, beside a wildcard endpoint |
| When | the guest sends requests carrying the placeholder in several ways |
| Then | only HTTPS to the exact host on an allowed, unambiguous path carries the secret. The placeholder must sit in the credential's header |

## `NW-051` A swap replaces the placeholder in its header only

| Attribute | Value |
| --- | --- |
| refines | `NW-050` |

| Step | Statement |
| --- | --- |
| Given | a credential's secret bound to a request |
| When | the default outbound seam builds the request's headers |
| Then | the placeholder is replaced in the credential's header only, never in another header, and the secret prints as a mask |

## `NW-052` A credential is never swapped into a request in clear

| Attribute | Value |
| --- | --- |
| refines | `NW-050` |

| Step | Statement |
| --- | --- |
| Given | a `#network` block binding a bearer credential to a loopback endpoint |
| When | the script sends the placeholder over plain HTTP through `Net::HTTP` |
| Then | the server reads the placeholder unchanged, and the credential is never asked for its value |

## `NW-053` A datagram round-trips with one peer

| Attribute | Value |
| --- | --- |
| relates | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a UDP socket connected to a loopback peer |
| When | it sends a datagram, and the peer answers one longer than the read |
| Then | the peer hears the datagram. The read takes one datagram cut to the buffer, and the rest of it is gone |

## `NW-054` A datagram stream passes only a UDP endpoint

| Attribute | Value |
| --- | --- |
| refines | `NW-013` |

| Step | Statement |
| --- | --- |
| Given | a `udp` endpoint naming one address and a `tcp` endpoint naming another |
| When | the guest opens a datagram stream to each |
| Then | the first passes, and the second answers `EACCES`, since an endpoint's protocol bounds what reaches it |

## `NW-055` A UDP socket streams only with a peer it names

| Attribute | Value |
| --- | --- |
| refines | `NW-053` |

| Step | Statement |
| --- | --- |
| Given | a UDP socket in a network whose `udp` endpoint names a loopback peer |
| When | it binds a fixed port, streams with no peer, then streams with the peer |
| Then | the bind and the peerless stream answer `access-denied`. The datagram stream to the peer carries datagrams both ways |

## `NW-056` A UDPSocket carries a datagram both ways with the peer it connects to

| Attribute | Value |
| --- | --- |
| refines | `NW-055` |

| Step | Statement |
| --- | --- |
| Given | a `#network` block naming a loopback peer as a `udp` endpoint |
| When | the script connects a `UDPSocket` to the peer, sends, and calls `recvfrom` |
| Then | it reads the peer's answer and the peer's address as CRuby's `["AF_INET", port, host, host]` |

## `NW-057` A UDPSocket with no named peer raises what Linux would

| Attribute | Value |
| --- | --- |
| refines | `NW-055` |

| Step | Statement |
| --- | --- |
| Given | a `UDPSocket` no `connect` has named a peer for |
| When | the script binds a chosen port, sends with no address, sends to an address, then binds port `0` |
| Then | the bind raises `Errno::EACCES` and the first send `Errno::EDESTADDRREQ`. The send to an address raises `Errno::EACCES`, and the bind to port `0` passes |

## `NW-058` A response body past the buffer streams as the guest reads

| Attribute | Value |
| --- | --- |
| refines | `NW-037` |

| Step | Statement |
| --- | --- |
| Given | a server answering with a body several times the socket buffer |
| When | the guest reads the body as it arrives |
| Then | it reads every byte in order, and the body waiting for the guest never passes one socket buffer |

## `NW-059` A run past its connection limit answers EAGAIN

| Attribute | Value |
| --- | --- |
| refines | `NW-022` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose `connection_limit` an invocation's sockets and requests already spent |
| When | the guest connects once more, though each earlier connection closed |
| Then | the connect answers `EAGAIN`, never a Trap, and dials nothing |

## `NW-060` A Rust Host App moves the process's open connection limit

| Attribute | Value |
| --- | --- |
| refines | `NW-022` |

| Step | Statement |
| --- | --- |
| Given | the process's open connections, holding some open |
| When | a Rust Host App lowers its limit below what is open, then lifts it |
| Then | each open connection stays open and the next is refused, until the lifted limit admits it |
