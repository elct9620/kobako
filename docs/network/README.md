# Network

The Network Context owns the named entries, their endpoints, and the sockets the guest holds. This document records the design that spans `kobako-runtime`, `kobako-wasmtime`, `kobako-io`, and the Ruby bridge. The whole map is in [`architecture.md`](../architecture.md).

| Document | Holds |
|---|---|
| this one | the outbound path, the socket file, virtual addresses, waiting, the answers without a Grant |
| [`spec/behavior/network.md`](../spec/behavior/network.md) | each promise, as a scenario |
| [`guides/security.md`](../guides/security.md) | what an endpoint lets the guest do |
| [`guides/customization.md`](../guides/customization.md) | the network seams a Host App replaces |

## Layers

A socket and a request enter apart, as WASI splits them, and meet at the socket layer. The L7 proxy dials its upstream through the socket layer, so one ruleset and one set of limits judge every connection.

```
                 L4                      L7
+- guest ---------------------------------------------+
|          +--------------+        +--------------+   |
|          | TCPSocket    |        | Net::HTTP    |   |
|          +------+-------+        +------+-------+   |
+-----------------+-----------------------+-----------+
                  | wasi:sockets          | wasi:http
                  v                       v
+- kobako-wasmtime -----------------------------------+
|          +--------------+        +--------------+   |
|          | WIT Encoder  |        | WIT Encoder  |   |
|          +------+-------+        +------+-------+   |
+-----------------+-----------------------+-----------+
                  | connect               | send
                  v                       v
+- kobako-runtime ------------------------------------+
|          +--------------+ dials  +--------------+   |
|          | Socket Layer |<-------| L7 Proxy     |   |
|          +------+-------+        +--------------+   |
+-----------------+-----------------------------------+
                  | dials
                  v
+- host ----------------------------------------------+
|          +--------------------------------------+   |
|          | Protocol Family                      |   |
|          +--------------------------------------+   |
+-----------------------------------------------------+
```

The Wasm Driver keeps its entry checks and the steps only WASI has, such as `start-*` and `finish-*`. Every Linux answer the guest sees is decided in `kobako-runtime`, so another Wasm Driver answers alike.

### Linux Roles

Linux's shape holds down to the per-family socket. Below it, the Protocol Family is a backend, as a Filesystem is below the VFS. gVisor's `hostinet` likewise hands the rest to a host socket.

| Part | L4 | L7 |
|---|---|---|
| guest | an application trying each address | an application over a host HTTP client |
| kobako-wasmtime | the syscall entry and fdtable | the same |
| kobako-runtime | sockfs, `proto_ops`, LSM, cgroup `connect4`, the filter | a userspace egress proxy |
| host | the network beneath, as gVisor's `hostinet` | the same, through the socket layer |

The L7 proxy has no kernel counterpart. It is the userspace proxy an egress-controlled host runs. Its upstream passes the socket layer again, as Cilium marks Envoy's upstream with the workload's identity.

### Replaceable Parts

A Host App replaces a part only where the rules above it still hold, whatever the part does. The engine's `Config` takes the backends, one pair for each Sandbox, as gVisor takes one network stack. The Rust SDK's `Options` and the gem's `Sandbox.new(network:)` hand them on.

| Part | Its rules sit | Host App |
|---|---|---|
| security modules, entries | are the rules, and only refuse | hands its own |
| Protocol Family | above it: it dials only an admitted address | hands its own |
| Resolver | above it: connect judges each answer again | hands its own |
| L7 proxy's sender | inside it: they hold only while it dials through the socket layer | keeps kobako's |

For a test, kobako's own Mock stands as both backends. It answers names from a table and dials only its routes, so a script naming a real host reaches a server the test runs. A lookup and an HTTP dial run off the guest thread, so the Mock never calls back into the Host App. A Ruby test writes it as data.

```
guest names api.example.com
  -> rules -> Mock table: 93.184.216.34 -> rules -> Mock route -> 127.0.0.1:port
```

### Socket Path

A connect passes the socket layer in Linux's order. One connect reaches one destination, so the guest tries each address a lookup handed it, as CRuby's `TCPSocket.new` does.

```
+--------------+  named  +-------------+  state ok  +------------+
| Socket Hooks |-------->| proto_ops   |----------->| connect4   |
+--------------+         | connect     |            +-----+------+
                         +-------------+                  | real
                                                          v
+--------------+  passed +-------------+   port     +------------+
| Protocol     |<--------| Filter      |<-----------| proto      |
| Family       |         +-------------+            | connect    |
+--------------+                                    +------------+
```

The state check comes before translation, as `__inet_stream_connect` checks `sock->state` before `pre_connect`. A refusal at the filter gives the port back.

### HTTP Path

The L7 proxy judges a request on the guest thread, where Ruby policies and credential sources run. From `Outbound` on, it runs on the reactor.

```
+---------------+  name  +-------------+  admits  +--------+
| Socket Hooks  |------->| HTTP Rules  |--------->| Swaps  |
+---------------+        +-------------+          +---+----+
                                                      | request
                                                      v
+--------------+ as Invocation +-----------+  sends +----------+
| Socket Layer |<--------------| Connector |<-------| Outbound |
+--------------+               +-----------+        +----------+
```

`Swaps` are the credentials a request over `https` carries. The connector resolves the authority and tries each admitted address in turn, as hyper-util's `HttpConnector` does. Each dial passes the socket layer as the Invocation, so the ruleset and the connection limits judge it as they judge a guest socket. A Wasm Driver of one's own may replace the proxy, but not the socket layer beneath it.

The request carries the authority the rules judged, so a server routing by `Host` reads what was admitted. The response head reaches the guest whole, where wasmtime strips its forbidden fields. The guest's `Net::HTTP` reads `transfer-encoding` to tell a sized body from a chunked one, as CRuby's does.

## Outbound Path

Every outbound connection takes Linux's order: decide on what the guest named, translate, then filter what is really reached.

1. Decode the call at the Wasm Driver.
2. Ask the socket hooks about what the guest named, then stop at a refusal.
3. Check the socket's state, as `proto_ops->connect` does.
4. Translate a virtual address to its one real address, as cgroup `connect4` does.
5. Take a virtual port and count the connection, as the protocol's connect does.
6. Filter the real address against the ruleset, giving the port back at a refusal.
7. Connect that exact address through the Protocol Family.

The hooks decide and never translate, as cgroup `connect4` runs after `security_socket_connect`. Step 7 never resolves again, which closes DNS rebinding.

### Hook Order

The socket hooks run in Kernel Assembly's order, and any refusal wins.

| Order | Judge | Sees |
|---|---|---|
| 1 | kobako's own security module, which binds the unspecified address at port 0 alone | what the guest named |
| 2 | the Host App's security modules | what the guest named |
| 3 | each entry's policies | what the guest named |
| 4 | each endpoint's `addresses`, at the filter | the real address |

A policy never sees a real address, so an address limit belongs to an endpoint's `addresses`.

## Socket File

A guest socket is one file with many views, as handles on one `struct file` share one `struct socket`.

```
tcp-socket ------+
input-stream ----+--> socket file --> proto ops --> host socket
output-stream ---+         |
pollable --------+         +--> wait queue <-- reactor
```

The file is made with the socket, as Linux's `socket()` installs one before any connect. It never waits, since WASI's sockets never block. Only preview2 reaches it: an older encoding adapts to a newer one, so preview1 numbers its files alone. The guest adapts its sockets itself, as wasi-libc's descriptor table does. Its `TCPSocket` and `UDPSocket` are each an `IO` that `IO.select` waits on, and each keeps its shut sides.

| When | The socket file |
|---|---|
| a read or write would wait | answers EAGAIN, whatever its flag |
| its last view drops with bytes queued | sends them, then its FIN, as `__tcp_close` does |
| its last view drops with bytes unread | resets the connection |
| the Invocation ends | lets go of whatever it still holds |

## Socket Options

A socket holds its options, as Linux's `struct sock` does, because the host socket exists only once a connect dials it. Each layer keeps the rules its own standard sets.

| Layer | Keeps |
|---|---|
| guest gem | CRuby's names and forms, and Linux's EINVAL bounds |
| Wasm Driver | WASI's rules: zero refused, a time clamped, a size read halved |
| `kobako-runtime` | the value, Linux's defaults and doubling, the host socket's copy |

A read answers the socket's own value, never the host's, so a host default such as its hop limit never reaches the guest. The options WASI defines are the only ones open; `TCP_NODELAY` and the rest answer `ENOPROTOOPT`.

## Virtual Addresses

The guest sees only virtual addresses, so the host's network never reaches it. Each virtual address stands for one real address, so translation is one to one, as cgroup `connect4` rewrites one destination.

| Call | Maps | Linux precedent |
|---|---|---|
| a lookup | each real address to a virtual one | — |
| a connect | virtual to real | cgroup `connect4` |
| `remote-address` | real to virtual | cgroup `getpeername4` |
| `local-address` | `198.19.255.254`, at a virtual port | `inet_sport` |
| a UDP receive | real to virtual | cgroup `recvmsg4` |
| a connect to an unmapped one | refused, `EACCES` | — |

The range is `198.18.0.0/15`, inside the reserved set, so no real destination lies in it. Each Invocation allocates from the bottom and never reuses an address. A name keeps its virtual addresses and the real addresses its first lookup found, as a fake-IP pool answers a known host. A later lookup meets the hooks and the endpoints again but queries nothing, so a resolver cannot rebind it. A lookup never hands out `198.19.255.254`, the guest's own end.

### Virtual Ports

A socket's own port is virtual too. The protocol takes it as it binds or connects, as `inet_csk_get_port` and `inet_hash_connect` do. Ports run from 32768, as `ip_local_port_range` starts. The host's ephemeral port stays with the Protocol Family, since it would carry the host's connection state into the guest, as CVE-2022-1012 showed.

| Socket | Its own end |
|---|---|
| unbound | `0.0.0.0` port 0 |
| bound to port 0 | `0.0.0.0` at a virtual port |
| connected | `198.19.255.254` at its virtual port |

### Names and Literals

`wasi:sockets` looks a name up and connects in two calls, so a virtual address carries the name from one to the other.

```
TCPSocket                 kobako-runtime            Resolver, Family
    |  resolve name           |                           |
    |------------------------>|  resolve name             |
    |                         |-------------------------->|
    |                         |<--------------------------|
    |  virtual addresses      |  real addresses           |
    |<------------------------|                           |
    |  connect each in turn   |                           |
    |------------------------>|  dial its real one        |
    |                         |-------------------------->|
```

The connect is judged by the name, and dials only what that lookup admitted, so a second resolution never happens.

| The guest names | Virtual address | Judged by |
|---|---|---|
| a name, over `wasi:sockets` | one per real address | the name, then that real address |
| a literal IP | none | endpoints naming that address |
| an HTTP authority | none | the name, then each real address, at the proxy |

An HTTP request carries the name to the host itself, so no address ever reaches the guest.

## Waiting

One reactor thread per process waits on every host socket, as a softirq fills a socket's queue and wakes its waiters.

| Waits | On |
|---|---|
| TCP and UDP I/O | the reactor |
| HTTP | hyper's HTTP/1 client, a task on the reactor |
| a name lookup | a thread of its own, as a resolver blocks |
| a guest's timeout | a clock the guest polls beside the socket |
| an HTTP request's timeout | the exchange's next deadline, read as the wait turns |

A lookup, a response, and a request body are each a file on one anonymous inode, as an eventfd is. A Wasm Driver therefore waits on files alone, and the profile decides when a clock beside them is due. The host keeps no timer: an exchange's timeout ends a wait by its deadline, which the exchange answers when asked. A timeout the request leaves unset lasts 600 seconds, as wasmtime-wasi-http's does, so a wait ends even in a run with no deadline. A wait the deadline ends traps the run, and the run's end releases every socket it held.

## Without a Grant

Every Guest Binary imports the network interfaces, so every Wasm Driver links them. Without a network Grant, each answers at once and does no work first.

| Call | Answer |
|---|---|
| `create-tcp-socket`, `create-udp-socket` | `access-denied` |
| `resolve-addresses` | `permanent-resolver-failure` |
| `outgoing-handler.handle` | `HTTP-request-denied` |

These are the answers wasmtime-wasi gives with sockets disallowed. The guest gem raises what CRuby raises for each: `Errno::EACCES` for a socket and a request, and `Socket::ResolutionError` for `EAI_FAIL` for a lookup.

## Guest Errors

A refusal reaches the script as the `Errno` CRuby's would raise, and the script goes on.

| Case | Guest sees |
|---|---|
| a name no endpoint allows | `Socket::ResolutionError` |
| an address, a policy, or a bind refused | `Errno::EACCES` |
| a UDP read with no peer, waiting or not | `Errno::EACCES` |
| an IPv6 family or literal | `Errno::EAFNOSUPPORT` |
| a full connection cap, at a connection | `Errno::EAGAIN` |
| a full connection cap, at a lookup | `Socket::ResolutionError` for `EAI_AGAIN` |
| the open file limit, at a new socket | `Errno::EMFILE` |
| a receive or send flag WASI carries none of, such as `MSG_OOB` | `Errno::ENOTSUP` |

A wait past the deadline traps as `TimeoutTrap`, not as an `Errno`. An endpoint the ruleset refuses is the Host App's argument error, raised before the guest runs.
