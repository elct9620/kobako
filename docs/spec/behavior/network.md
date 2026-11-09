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
- `test/e2e/test_network_tcp_cruby.rb`
- `test/e2e/test_network_tcp_arguments.rb`
- `test/e2e/test_network_own_end.rb`
- `test/e2e/test_network_socket_io.rb`
- `test/e2e/test_network_names.rb`
- `test/e2e/test_network_http.rb`
- `test/e2e/test_network_http_cruby.rb`
- `test/e2e/test_network_http_headers.rb`
- `test/e2e/test_network_http_session.rb`
- `test/e2e/test_network_http_timeout.rb`
- `test/e2e/test_network_http_policy.rb`
- `test/e2e/test_network_https.rb`
- `test/e2e/test_network_udp.rb`
- `test/e2e/test_network_udp_cruby.rb`
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
- `crates/kobako-wasmtime/src/preview2/sockets.rs`

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

## `NW-061` A script's Net::HTTP streams a body through a block

| Attribute | Value |
| --- | --- |
| refines | `NW-042` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming a server whose body is larger than the guest's `memory_limit` |
| When | the script reads it through `read_body` in `request`'s block, or through `get`'s block |
| Then | each chunk reaches the block in order and the run stays within its memory. A second `read_body` raises `IOError` as CRuby's does. `body` after a streamed read answers `nil`, where CRuby answers a `Net::ReadAdapter` |

## `NW-062` A script's Net::HTTP helpers answer as CRuby's

| Attribute | Value |
| --- | --- |
| refines | `NW-042` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming a loopback server |
| When | the script builds a request with `basic_auth` and `set_form_data`, sends it, and reads the response's helpers |
| Then | the server reads the form and the Basic credentials. `content_type`, `key?`, `delete`, `content_length`, `header`, and `code_type` answer as CRuby's, and `http_version` answers `"1.1"` |

## `NW-063` A failed response's value raises what CRuby raises

| Attribute | Value |
| --- | --- |
| refines | `NW-042` |

| Step | Statement |
| --- | --- |
| Given | a server answering a client or server error |
| When | the script calls `value` on the response |
| Then | it raises the `Net::HTTPExceptions` error CRuby raises for that family, carrying the response |

## `NW-064` A wasi:http parent dropped while its child lives traps

| Attribute | Value |
| --- | --- |
| refines | `NW-037` |

| Step | Statement |
| --- | --- |
| Given | an `outgoing-body` whose `output-stream` the guest still holds |
| When | it drops or finishes the body |
| Then | the call traps, as the WIT traps a parent dropped before its child. Once the stream is dropped, the body finishes |

## `NW-065` A script's TCPSocket is an IO that IO.select waits on

| Attribute | Value |
| --- | --- |
| refines | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a TCP endpoint naming a loopback server that answers once the script closes its write side |
| When | the script selects the socket to read, closes its write side, and selects again. Then it selects `$stdout` and the socket to write |
| Then | `TCPSocket` descends from `IPSocket`, `BasicSocket`, and `IO`, and its `fileno` is an Integer. The first select times out with `nil`. The second answers the socket readable, and the third answers both writable |

## `NW-066` An IPv6 family or literal raises Errno::EAFNOSUPPORT

| Attribute | Value |
| --- | --- |
| refines | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a network entry, and a guest that speaks only IPv4 |
| When | the script opens a `TCPSocket` to `::1`, or a `UDPSocket` of `Socket::AF_INET6` |
| Then | each raises `Errno::EAFNOSUPPORT`, as Linux refuses a family it does not carry |

## `NW-067` A script's TCPSocket takes CRuby's call shapes

| Attribute | Value |
| --- | --- |
| refines | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a TCP endpoint naming a loopback server that answers once the script closes its write side |
| When | the script connects with `connect_timeout:`, writes several strings in one call, and reads with `gets(sep)` and `readpartial(n, buf)` |
| Then | each answers as CRuby's does. A local port the script chooses raises `Errno::EACCES`, and an unknown keyword raises `ArgumentError` |

## `NW-068` A script's Net::HTTP raises Net::ReadTimeout past its read timeout

| Attribute | Value |
| --- | --- |
| refines | `NW-037` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming a loopback server that accepts and never answers |
| When | the script sends a GET with `read_timeout` set to a tenth of a second |
| Then | it raises `Net::ReadTimeout`, a `Timeout::Error`, once the limit passes. Each timeout starts at 60 seconds, as CRuby's does. The connect counts against `read_timeout`, since `wasi:http` shows the guest no connect apart |

## `NW-069` A socket past the open file limit answers new-socket-limit

| Attribute | Value |
| --- | --- |
| relates | `RT-341` |

| Step | Statement |
| --- | --- |
| Given | a network entry, and a guest holding as many files as its open file limit allows |
| When | it creates a TCP or a UDP socket. With one file left, it creates a TCP socket and connects it |
| Then | the full limit answers `new-socket-limit`, which the guest raises as `Errno::EMFILE`, as Linux's `socket()` answers. With one file left, the socket connects and hands out its streams, as Linux's `connect` takes no fd. Nothing traps |

## `NW-070` A UDP socket's failure is answered once

| Attribute | Value |
| --- | --- |
| refines | `NW-053` |

| Step | Statement |
| --- | --- |
| Given | a UDP socket whose peer's port is closed |
| When | it sends a datagram and then reads twice |
| Then | the first read answers ECONNREFUSED and the second reads on, as Linux's `sock_error` clears the error it reports |

## `NW-071` An outgoing body dropped unfinished fails its request

| Attribute | Value |
| --- | --- |
| refines | `NW-037` |

| Step | Statement |
| --- | --- |
| Given | a request whose `outgoing-body` the guest holds, handed to `handle` |
| When | the guest drops the body without finishing it |
| Then | the response's future answers `HTTP-protocol-error`, as wasmtime-wasi-http aborts the body, rather than wait on |

## `NW-072` A datagram send past its permit traps

| Attribute | Value |
| --- | --- |
| refines | `NW-053` |

| Step | Statement |
| --- | --- |
| Given | an outgoing datagram stream with its peer |
| When | the guest sends before any `check-send`, then after one, then again |
| Then | the first and the last trap, as `udp.wit` says a send past the permit must, and the middle one sends |

## `NW-073` A blocking write past 4096 bytes traps

| Attribute | Value |
| --- | --- |
| refines | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a connected socket's output stream |
| When | the guest hands `blocking-write-and-flush` 4097 bytes, then 4096 |
| Then | the first traps, as `io.wit` bounds the call, and the second writes |

## `NW-074` A host failure answers the code wasmtime-wasi gives it

| Attribute | Value |
| --- | --- |
| relates | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a host socket failure with a POSIX name |
| When | the Wasm Driver answers it as a `wasi:sockets` error code |
| Then | EPIPE answers `unknown`, EADDRINUSE `address-in-use`, and ECONNRESET `connection-reset`, as wasmtime-wasi's preview2 maps them |

## `NW-075` An endpoint names an IDN by its A-label

| Attribute | Value |
| --- | --- |
| refines | `NW-033` |

| Step | Statement |
| --- | --- |
| Given | endpoints written as an IDN, and as a wildcard over one |
| When | the guest looks up the A-label of a name each covers |
| Then | each endpoint names it, since an endpoint is judged in the form the guest's lookup is |

## `NW-076` A credential's placeholder is swapped inside a Basic pair

| Attribute | Value |
| --- | --- |
| refines | `NW-051` |

| Step | Statement |
| --- | --- |
| Given | a credential bound to the `authorization` header |
| When | a request carries `Basic` and a pair encoding the placeholder |
| Then | the pair is decoded, the placeholder replaced, and the pair encoded again, as OpenShell's does. A pair without it goes as sent |

## `NW-077` A request is judged once, in order, and never over IPv6

| Attribute | Value |
| --- | --- |
| refines | `NW-021` |

| Step | Statement |
| --- | --- |
| Given | an entry with endpoints, policies, and a credential |
| When | the guest sends a request a connect hook refuses, and one to an IPv6 authority an endpoint names |
| Then | the entry's endpoints were read once. No resolve policy is asked once a hook refused. The IPv6 one is refused, as the guest has no IPv6 |

## `NW-078` A duck-typed endpoint takes the defaults and an IPAddr

| Attribute | Value |
| --- | --- |
| relates | `NW-008` |

| Step | Statement |
| --- | --- |
| Given | a Ruby entry whose endpoints are objects answering `host` and `port`, and `addresses` for one of them |
| When | the script sends a request to each |
| Then | the one answering no `addresses` leads only to public addresses, as an endpoint written without them does. The one naming an `IPAddr` speaks `:http` and leads where the prefix covers |

## `NW-079` A script's UDPSocket is an IO that IO.select waits on

| Attribute | Value |
| --- | --- |
| refines | `NW-056` |
| relates | `NW-065` |

| Step | Statement |
| --- | --- |
| Given | a UDP endpoint naming a loopback peer |
| When | the script selects a new `UDPSocket` to write and writes to it. Then it connects the socket to the peer and writes two strings |
| Then | `UDPSocket` descends from `IPSocket`, `BasicSocket`, and `IO`, and its `fileno` is an Integer. The select answers the socket writable. The first write raises `Errno::EDESTADDRREQ`, as CRuby's does. The peer reads the two strings as one datagram |

## `NW-080` A script's TCPSocket carries no TLS of its own

| Attribute | Value |
| --- | --- |
| relates | `NW-048`, `NW-065` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a network Grant |
| When | the script names `OpenSSL::SSL::SSLSocket` to wrap a `TCPSocket` |
| Then | it raises `NameError`, where CRuby wraps the socket, since the guest has no TLS stack. TLS reaches the script only through an `https` request, which the host ends |

## `NW-081` A header list past its room traps

| Attribute | Value |
| --- | --- |
| refines | `NW-041` |

| Step | Statement |
| --- | --- |
| Given | a guest building headers through `wasi:http` |
| When | it appends past 128 KiB of names and values, or past its `memory_limit` |
| Then | the call traps, as wasmtime's field limit traps, since `header-error` has no case for it. A list holds its room against the Invocation's memory until it drops |

## `NW-082` An HTTP exchange the guest no longer holds closes its connection

| Attribute | Value |
| --- | --- |
| relates | `NW-040`, `NW-059` |

| Step | Statement |
| --- | --- |
| Given | an `http` endpoint naming a server that sends a response's head and holds its body open |
| When | the guest drops the response's body without finishing it, or the Invocation ends while the guest holds it |
| Then | the server reads the connection closed. Nothing of the exchange outlives what the guest holds of it |

## `NW-083` What a network policy raises beyond StandardError ends the run

| Attribute | Value |
| --- | --- |
| refines | `NW-021` |
| relates | `T-282`, `MT-023` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose network entry's `connect?` policy raises `Interrupt` |
| When | the script opens a `TCPSocket` its endpoint names and rescues every failure |
| Then | the run raises `Interrupt` in the Host App, as a Service's or a mount's `Interrupt` does |

## `NW-084` A datagram past what UDP carries answers datagram-too-large

| Attribute | Value |
| --- | --- |
| refines | `NW-056` |

| Step | Statement |
| --- | --- |
| Given | a UDP socket streaming to the peer its endpoint names |
| When | the guest sends one datagram a byte past what IPv4 UDP carries |
| Then | the send answers `datagram-too-large`, as Linux's `udp_sendmsg` answers `EMSGSIZE` |

## `NW-085` A request's name lookup holds its place until the resolver returns

| Attribute | Value |
| --- | --- |
| refines | `NW-059` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose `open_connection_limit` is 1, and a host resolver that has not answered |
| When | the guest releases a request whose authority is still resolving, then sends another |
| Then | the second answers `connection-limit-reached` until the resolver returns, since the lookup's thread holds the first request's place |

## `NW-086` A zero-byte read keeps a UDP socket's datagram

| Attribute | Value |
| --- | --- |
| refines | `NW-053` |

| Step | Statement |
| --- | --- |
| Given | a UDP socket holding a datagram from its peer |
| When | the guest reads zero bytes from the socket's file, then reads again |
| Then | the first read answers zero and takes nothing, as `sock_read_iter` does, and the second reads the datagram |

## `NW-087` A UDP socket answers its failure before what it holds

| Attribute | Value |
| --- | --- |
| refines | `NW-070` |

| Step | Statement |
| --- | --- |
| Given | a UDP socket holding a datagram and a failure its host reported |
| When | the guest polls the socket's file, then reads twice |
| Then | the poll finds IN and ERR. The first read answers the failure, as `__skb_recv_udp` asks `sock_error` first, and the second the datagram |

## `NW-088` A UDP socket connected again keeps its place

| Attribute | Value |
| --- | --- |
| refines | `NW-022` |

| Step | Statement |
| --- | --- |
| Given | a UDP socket connected under a cap of one connection |
| When | it connects to another peer |
| Then | the connect succeeds, since the socket holds the place it took, as `__ip4_datagram_connect` repoints a socket |

## `NW-089` An address WASI calls invalid answers invalid-argument before any hook

| Attribute | Value |
| --- | --- |
| refines | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a network Grant |
| When | it connects or streams to the unspecified address, port 0, a broadcast, or a multicast address. It binds a UDP socket to a multicast address |
| Then | each answers `invalid-argument` before the socket hooks are asked, as wasmtime's checks answer them |

## `NW-090` A second start while one is under way answers concurrency-conflict

| Attribute | Value |
| --- | --- |
| refines | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a TCP socket whose connect started, and a UDP socket whose bind started, neither finished |
| When | the guest starts the same again on each |
| Then | each answers `concurrency-conflict`, as `sockets.wit` names a call made while another is in progress |

## `NW-091` A TCP socket's pollable waits only on its connect

| Attribute | Value |
| --- | --- |
| refines | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a TCP socket that never connected, and one whose connect finished |
| When | the guest blocks on a pollable each `subscribe` hands out |
| Then | each answers at once, since only a connect under way makes the pollable wait, as wasmtime's socket pollable does |

## `NW-092` A write past the permit check-write gave traps

| Attribute | Value |
| --- | --- |
| refines | `NW-073` |

| Step | Statement |
| --- | --- |
| Given | a connected socket's output stream, whose `check-write` permitted some bytes |
| When | the guest writes those bytes, then one more without asking `check-write` again |
| Then | the first write succeeds and the second traps, as `io.wit` says a write past the last permit must |

## `NW-093` A request's parts are judged as they are set

| Attribute | Value |
| --- | --- |
| refines | `NW-040` |

| Step | Statement |
| --- | --- |
| Given | a guest building a request through `wasi:http` |
| When | it sets an authority with a bad port, the paths `""` and `"?q"`, and a header with a control byte |
| Then | the authority and the header answer an error as they are set, as wasmtime's do. The paths are taken as `/` and `/?q` |

## `NW-094` A response's header list refuses a change before it reads the change

| Attribute | Value |
| --- | --- |
| refines | `NW-041` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a response's headers, which it cannot change |
| When | it appends a header whose name is no token |
| Then | the append answers `invalid-syntax`, as wasmtime reads a header before it finds the list immutable |

## `NW-095` A script's socket reads through what it holds, as CRuby's IO does

| Attribute | Value |
| --- | --- |
| refines | `NW-065`, `NW-079` |

| Step | Statement |
| --- | --- |
| Given | a TCP endpoint naming a loopback server that answers two lines, and a UDP peer that answers datagrams |
| When | the script reads the TCP socket with `eof?`, `gets(chomp: true)`, `read(n, buf)`, `each_line`, and `readlines`. Then it reads the UDP socket with `read` |
| Then | each answers as CRuby's does, and a call takes what an earlier call held back first. A UDP read takes one whole datagram |

## `NW-096` A script's socket has no position, sync, or copy

| Attribute | Value |
| --- | --- |
| refines | `NW-065`, `NW-079` |

| Step | Statement |
| --- | --- |
| Given | a script holding a `TCPSocket` and a `UDPSocket` |
| When | it calls `seek`, `pos`, `fsync`, `advise`, `sync=`, and `dup` on each |
| Then | the position calls raise `Errno::ESPIPE`, the sync calls `Errno::EINVAL`, and `advise` answers `nil`, as Linux does. `sync` reads back what `sync=` set. `dup` and `clone` raise `NoMethodError`, where CRuby copies the socket, since WASI copies no descriptor |

## `NW-097` A reset stream read raises Errno::ECONNRESET

| Attribute | Value |
| --- | --- |
| refines | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a TCP endpoint naming a loopback server that resets the connection |
| When | the script reads the socket |
| Then | the read raises `Errno::ECONNRESET`, as CRuby's does |

## `NW-098` A script's UDPSocket refuses what CRuby's refuses

| Attribute | Value |
| --- | --- |
| refines | `NW-079` |
| relates | `NW-066` |

| Step | Statement |
| --- | --- |
| Given | a network entry, and a guest that speaks only IPv4 |
| When | the script opens a `UDPSocket` of a family other than `Socket::AF_INET`. Then it calls `new`, `send`, and `recv` with a wrong argument count |
| Then | the family raises `Errno::EAFNOSUPPORT`, and each count raises `ArgumentError` naming the range CRuby's takes. `Socket.new` raises `NoMethodError`, where CRuby opens a socket, since a script opens only a `TCPSocket` or a `UDPSocket` |

## `NW-099` A script's port is read as Linux reads a service

| Attribute | Value |
| --- | --- |
| refines | `NW-067` |
| relates | `NW-056` |

| Step | Statement |
| --- | --- |
| Given | a TCP endpoint naming a loopback server |
| When | the script opens a `TCPSocket` to the server's port as a String, and as an Integer past 16 bits. Then it names a service |
| Then | the String and the Integer reach the port Linux's `getaddrinfo` reads from them. The service name raises `Socket::ResolutionError` with `EAI_SERVICE`, where CRuby looks it up, since the guest has no services database |

## `NW-100` A script's TCPSocket bounds its name lookup by resolv_timeout

| Attribute | Value |
| --- | --- |
| refines | `NW-067` |

| Step | Statement |
| --- | --- |
| Given | a TCP endpoint naming `localhost` |
| When | the script opens a `TCPSocket` to it with `resolv_timeout:` shorter than the lookup, and again with `fast_fallback: false` |
| Then | the first raises `Errno::ETIMEDOUT`, as CRuby's does. The second connects, since a guest that speaks only IPv4 has no fallback to choose |

## `NW-101` A script's peeraddr takes CRuby's reverse lookup flags

| Attribute | Value |
| --- | --- |
| refines | `NW-067` |

| Step | Statement |
| --- | --- |
| Given | a script holding a connected `TCPSocket` |
| When | it calls `peeraddr` with `true`, `:hostname`, `:numeric`, an unknown Symbol, and an Integer |
| Then | the unknown Symbol raises `ArgumentError` and the Integer `TypeError`, as CRuby's do. A lookup asked for answers the address, where CRuby answers the host name, since the guest has no reverse lookup |

## `NW-102` A script's request carries CRuby's default headers

| Attribute | Value |
| --- | --- |
| refines | `NW-062` |

| Step | Statement |
| --- | --- |
| Given | an HTTP endpoint naming a loopback server |
| When | the script sends a GET, and a POST whose body names no content type |
| Then | each carries `accept: */*` and `user-agent: Ruby`, and the POST carries `application/x-www-form-urlencoded`, as CRuby's do. No `accept-encoding` is sent, where CRuby asks for gzip, since the guest decodes no content |

## `NW-103` A response that carries no body answers nil

| Attribute | Value |
| --- | --- |
| refines | `NW-062` |

| Step | Statement |
| --- | --- |
| Given | an HTTP endpoint naming a loopback server |
| When | the script sends a HEAD, and a GET the server answers with 204 or 304 |
| Then | each response's `body` answers `nil`, as CRuby's does |

## `NW-104` A request's headers are taken as CRuby takes them

| Attribute | Value |
| --- | --- |
| refines | `NW-062` |

| Step | Statement |
| --- | --- |
| Given | a script building a `Net::HTTP::Get` |
| When | it passes headers padded, `nil`, repeated in another case, and holding CR or LF. Then it sets an Array |
| Then | each answers as CRuby's does. The value is stripped, `nil` is skipped, and the last of a name wins. CR or LF raises `ArgumentError`, and an Array joins with `, ` |

## `NW-105` A header the host writes or WASI refuses raises ArgumentError

| Attribute | Value |
| --- | --- |
| refines | `NW-041` |
| relates | `NW-102` |

| Step | Statement |
| --- | --- |
| Given | an HTTP endpoint naming a loopback server |
| When | the script sends a request naming `host`, `connection`, or a name that is no token |
| Then | each raises `ArgumentError` before anything is sent, where CRuby sends it. The host writes the authority, so a script names no other host |

## `NW-106` A request's form data is encoded as CRuby encodes it

| Attribute | Value |
| --- | --- |
| refines | `NW-062` |

| Step | Statement |
| --- | --- |
| Given | a script holding a `Net::HTTP::Post` |
| When | it calls `set_form_data` with an Array value, a `nil` value, a Symbol, and a separator |
| Then | the body is what `URI.encode_www_form` writes, joined by the separator. The call answers the content type, as CRuby's does |

## `NW-107` A session and a request refuse what CRuby's refuse

| Attribute | Value |
| --- | --- |
| refines | `NW-062` |

| Step | Statement |
| --- | --- |
| Given | an HTTP endpoint naming a loopback server |
| When | the script starts a session twice, finishes one never started, and sends a request holding a body with another |
| Then | the session calls raise `IOError` and the two bodies `ArgumentError`, as CRuby's do. A path that is `nil` or empty raises `ArgumentError` |

## `NW-108` A script's Net::HTTP objects read as CRuby's

| Attribute | Value |
| --- | --- |
| refines | `NW-062` |

| Step | Statement |
| --- | --- |
| Given | an HTTP endpoint naming a loopback server |
| When | the script sets `use_ssl` after `new`, and calls `Net::HTTP.start` with `use_ssl: true`. Then it inspects each object |
| Then | the first keeps port 80 and the second takes 443, as CRuby's do. Each `inspect` answers CRuby's form, a response unread within its block as `readbody=false` |

## `NW-109` A script's Net::HTTP is a subset without URI or Enumerator

| Attribute | Value |
| --- | --- |
| refines | `NW-062` |

| Step | Statement |
| --- | --- |
| Given | an HTTP endpoint naming a loopback server |
| When | the script calls `URI`, `each_header` with no block, and a request with a relative path. Then it reads a response's `message` |
| Then | `URI` raises `NoMethodError` and `each_header` raises `NotImplementedError`, as the guest has no URI or Fiber. The relative path raises `ArgumentError`, where CRuby sends it. `message` answers HTTP/1.1's reason phrase, as WASI carries only the code |

## `NW-110` IO.select takes what converts to an IO

| Attribute | Value |
| --- | --- |
| refines | `NW-065` |

| Step | Statement |
| --- | --- |
| Given | a script holding a `TCPSocket`, an object whose `to_io` answers it, and an Integer |
| When | it selects the object, then the Integer |
| Then | the object is answered ready as it was given, and the Integer raises `TypeError`, as CRuby's `IO.select` does |

## `NW-111` A Context replaces an entry with what a builder adds to it

| Attribute | Value |
| --- | --- |
| refines | `NW-006` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a network entry named `api` |
| When | an invocation's Context replaces it with another entry and a builder adding an endpoint |
| Then | that invocation receives the other entry's endpoints, then the builder's |

## `NW-112` A script's socket reads its own end at the guest's virtual address

| Attribute | Value |
| --- | --- |
| refines | `NW-067`, `NW-079` |
| relates | `NW-031` |

| Step | Statement |
| --- | --- |
| Given | a TCP and a UDP endpoint naming loopback peers |
| When | the script reads `addr` on each socket, the UDP one before it connects, after, and after it connects again. Then it connects to that address |
| Then | each connected socket answers `198.19.255.254` and a port from 32768 up in connect order, never the host's. A second connect keeps the port. The unconnected one answers `0.0.0.0` port 0, and the connect raises `Errno::EACCES` |

## `NW-113` A socket dropped while its streams live traps

| Attribute | Value |
| --- | --- |
| refines | `NW-016`, `NW-053` |
| relates | `NW-064` |

| Step | Statement |
| --- | --- |
| Given | a connected TCP socket and a streaming UDP socket, each with its streams and a pollable still held |
| When | the guest drops a socket, or streams the UDP socket again |
| Then | the call traps, as wasmtime-wasi traps a parent dropped first and a second stream while the first lives. Once the children are dropped, each succeeds |
