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
- `test/unit/catalog/test_network.rb`
- `crates/kobako/tests/integration/network.rs`
- `crates/kobako-runtime/src/network.rs`
- `crates/kobako-runtime/src/network/builder.rs`
- `crates/kobako-runtime/src/network/ruleset.rs`
- `crates/kobako-runtime/src/net/namespace.rs`
- `crates/kobako-runtime/src/net/workers.rs`
- `crates/kobako-wasmtime/src/preview2/host.rs`

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

## `NW-022` A full worker cap answers EAGAIN

| Attribute | Value |
| --- | --- |
| relates | `NW-016` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose sockets' workers fill its cap, or the process's |
| When | the guest connects once more |
| Then | the connect answers `EAGAIN`, never a Trap, and dials nothing. A worker that ends gives its place back, and a forked child starts from none |

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

## `NW-026` A virtual address no lookup handed out answers ENETUNREACH

| Attribute | Value |
| --- | --- |
| refines | `NW-025` |

| Step | Statement |
| --- | --- |
| Given | a guest that looked up no name |
| When | it connects to an address in the virtual range |
| Then | the connect answers `ENETUNREACH`, and nothing is dialed |

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
