# Network

The Network Context owns the named entries, their endpoints, and the sockets the guest holds. This document records the design that spans `kobako-runtime`, `kobako-wasmtime`, `kobako-io`, and the Ruby bridge. The whole map is in [`architecture.md`](../architecture.md).

| Document | Holds |
|---|---|
| this one | the outbound path, the socket file, virtual addresses, waiting, the answers without a Grant |
| [`spec/behavior/network.md`](../spec/behavior/network.md) | each promise, as a scenario |
| [`guides/security.md`](../guides/security.md) | what an endpoint lets the guest do |
| [`guides/customization.md`](../guides/customization.md) | the network seams a Rust Host App replaces |

## Layers

A socket and a request take two paths through the same four parts, and meet again at the Protocol Family.

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
|          +--------------+        +--------------+   |
|          | Socket Path  |        | HTTP Path    |   |
|          +------+-------+        +------+-------+   |
+-----------------+-----------------------+-----------+
                  | dials                 | dials
                  v                       v
+- host ----------------------------------------------+
|          +--------------------------------------+   |
|          | Protocol Family                      |   |
|          +--------------------------------------+   |
+-----------------------------------------------------+
```

The Wasm Driver keeps its entry checks and the steps only WASI has, such as `start-*` and `finish-*`. Every Linux answer the guest sees is decided in `kobako-runtime`, so another Wasm Driver answers alike.

### Linux Roles

Each part mirrors a Linux role, so a rule lands where Linux puts it. The HTTP Path has no kernel counterpart; it is the egress proxy a Linux sandbox runs in userspace.

| Part | L4 | L7 |
|---|---|---|
| guest | an application over socket calls | an application over a host HTTP client |
| kobako-wasmtime | the syscall entry and fdtable | the same |
| kobako-runtime | LSM, cgroup `connect4`, nftables, `struct sock` | an egress proxy, as OpenShell's supervisor |
| host | the network beneath, as gVisor's `hostinet` | the same, behind a replaceable sender |

### Socket Path

A socket's connect passes the socket hooks, the virtual address map, and the ruleset before the socket dials.

```
+---------------+  name  +----------------+  real   +-----------+
| Socket Hooks  |------->| Virtual Map    |-------->| Ruleset   |
+---------------+        +----------------+         +-----+-----+
                                                          | admitted
                                                          v
                                                    +-----------+
                                                    | TcpSock   |
                                                    +-----+-----+
                                                          | one address
                                                          v
                                                    +-----------+
                                                    | Protocol  |
                                                    | Family    |
                                                    +-----------+
```

The socket dials one admitted address at a time and moves to the next when one refuses. The reactor sees each refusal, so the socket owns this loop.

### HTTP Path

A request is judged on the guest thread, where Ruby policies and credential sources run. From `Outbound` on, it runs on the reactor.

```
+---------------+  name  +-------------+  admits  +--------+
| Socket Hooks  |------->| HTTP Rules  |--------->| Swaps  |
+---------------+        +-------------+          +---+----+
                                                      | request
                                                      v
+-----------+  filters  +-------------+  dials   +----------+
| Ruleset   |<----------| Connector   |<---------| Outbound |
+-----------+           +------+------+          +----------+
                               | one address
                               v
                        +-------------+
                        | Protocol    |
                        | Family      |
                        +-------------+
```

`Swaps` are the credentials a request over `https` carries. A Rust Host App may replace the outbound sender, which still dials through the `Connector`, so the same ruleset judges it. The connector resolves the authority itself and tries each admitted address, as hyper-util's `HttpConnector` does.

## Outbound Path

Every outbound connection takes Linux's order: decide on what the guest named, translate, then filter what is really reached.

1. Decode the call at the Wasm Driver.
2. Ask the socket hooks about what the guest named, then stop at a refusal.
3. Translate a virtual address to its name's real addresses.
4. Filter each real address against the ruleset.
5. Connect the exact address that passed, through the Protocol Family.

The hooks decide and never translate, as cgroup `connect4` runs after `security_socket_connect`. Step 5 never resolves again, which closes DNS rebinding.

### Hook Order

The socket hooks run in Kernel Assembly's order, and any refusal wins.

| Order | Judge | Sees |
|---|---|---|
| 1 | kobako's own security module, which binds the unspecified address at port 0 alone | what the guest named |
| 2 | the Host App's security modules | what the guest named |
| 3 | each entry's policies | what the guest named |
| 4 | each endpoint's `addresses` | each real address |

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

The guest sees only virtual addresses, so the host's network never reaches it.

| Call | Maps | Linux precedent |
|---|---|---|
| a lookup | a name to a virtual address | — |
| `remote-address` | real to virtual | cgroup `getpeername4` |
| `local-address` | `198.19.255.254`, at a virtual port | `ip_local_port_range` |
| a UDP receive | real to virtual | cgroup `recvmsg4` |
| a connect to an unmapped one | refused, `EACCES` | — |

The range is `198.18.0.0/15`, inside the reserved set, so no real destination lies in it. Each Invocation allocates from the bottom and never reuses an address for a second name. A name keeps its address and its first real addresses, as a fake-IP pool answers a known host. A later lookup meets the hooks and the endpoints again but queries nothing, so a resolver cannot rebind it. A lookup never hands out `198.19.255.254`, the guest's own end.

A connected socket's own port is virtual too, numbered from 32768 in connect order. The host's ephemeral port would carry its connection state into the guest, as CVE-2022-1012 showed. The socket keeps both ends, as a NAT's tuple does.

### Names and Literals

`wasi:sockets` looks a name up and connects in two calls, so a virtual address carries the name from one to the other.

```
TCPSocket                 Namespace                 Protocol Family
    |  resolve name           |                           |
    |------------------------>|  resolve name             |
    |                         |-------------------------->|
    |                         |<--------------------------|
    |  virtual address        |  real addresses           |
    |<------------------------|                           |
    |  connect virtual        |                           |
    |------------------------>|  dial real                |
    |                         |-------------------------->|
```

The connect is judged by the name, and dials only what that lookup admitted, so a second resolution never happens.

| The guest names | Virtual address | Judged by |
|---|---|---|
| a name, over `wasi:sockets` | handed out by the lookup | the name, then each real address |
| a literal IP | none | endpoints naming that address |
| an HTTP authority | none | the name, then each real address, in one call |

An HTTP request carries the name to the host itself, so no address ever reaches the guest.

## Waiting

One reactor thread per process waits on every host socket, as a softirq fills a socket's queue and wakes its waiters.

| Waits | On |
|---|---|
| TCP and UDP I/O | the reactor |
| HTTP | hyper's HTTP/1 client, a task on the reactor |
| a name lookup | a thread of its own, as a resolver blocks |
| a guest's timeout | a clock the guest polls beside the socket |

A lookup, a response, and a request body are each a file on one anonymous inode, as an eventfd is. A Wasm Driver therefore waits on files alone, and the profile decides when a clock beside them is due. The host keeps no timer. A wait the deadline ends traps the run, and the run's end releases every socket it held.

## Without a Grant

Every Guest Binary imports the network interfaces, so every Wasm Driver links them. Without a network Grant, each answers at once and does no work first.

| Call | Answer |
|---|---|
| `create-tcp-socket`, `create-udp-socket` | `not-supported` |
| `resolve-addresses` | `permanent-resolver-failure` |
| `outgoing-handler.handle` | `HTTP-request-denied` |

The guest gem raises `NotImplementedError` for each, as it does without a Mount.

## Guest Errors

A refusal reaches the script as the `Errno` CRuby's would raise, and the script goes on.

| Case | Guest sees |
|---|---|
| a name no endpoint allows | `Socket::ResolutionError` |
| an address, a policy, or a bind refused | `Errno::EACCES` |
| a UDP read with no peer, waiting or not | `Errno::EACCES` |
| an IPv6 family or literal | `Errno::EAFNOSUPPORT` |
| a full connection cap | `Errno::EAGAIN` |
| the open file limit, at a new socket | `Errno::EMFILE` |
| a receive or send flag WASI carries none of, such as `MSG_OOB` | `Errno::ENOTSUP` |

A wait past the deadline traps as `TimeoutTrap`, not as an `Errno`.
