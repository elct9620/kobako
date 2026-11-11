# Glossary

The words this project keeps, and the ones it turns down in their place.

## Everywhere

### Includes

- `README.md`
- `docs/**/*.md`
- `lib/**/*.rb`
- `ext/**/*.rs`
- `crates/**/*.rs`
- `wasm/**/*.rs`
- `examples/**/*.rb`
- `examples/**/*.md`

### Host App

The application that embeds kobako. It holds every credential and policy decision, and chooses which of its own objects the guest may reach.

### Host Gem

The Ruby gem: the Ruby Frontend and the native extension beneath it. The Rust SDK is the other Frontend, so the host side as a whole is the host.

### Guest Binary

The compiled artifact untrusted code runs inside. It is the isolation boundary: nothing crosses it except as a message.

### Guest

The contract a Guest Binary implements toward the host. mruby is one implementation of it, and any other that keeps it is indistinguishable to the host.

### Frontend

An API a Host App drives the guest through. The Ruby gem and the Rust SDK are two Frontends held to the same behavior. Each keeps its own language's idioms.

### Sandbox

The unit a Host App configures once and invokes many times. It owns a Catalog and keeps nothing from one Invocation to the next.

### Snippet

Guest source or bytecode preloaded on a Sandbox and replayed at the start of every Invocation.

### Bytecode

The compiled form of guest source, in a format the Guest defines. The host carries it as opaque bytes and never reads it. mruby's Guest uses RITE.

```
source ──Guest compiles──> Bytecode ──host replays unchanged──> Guest loads
```

### Service

A host object the guest reaches by name. Guest code reaches a host resource only through a Service or a Grant.

```
guest code ──by name─────────────> Service
           ──system interface────> Grant
```

#### Rejected

- `adapter` - Names a translation role. What distinguishes a Service is that the guest can name it at all.

### Handle

An opaque reference the guest holds to a host object the Codec cannot carry by value. It names that object only within the Invocation that issued it. Where the capability it grants is the point, it is called a Capability Handle.

### Transport

The exchange of messages across the boundary. One Call is answered by one Reply, and both directions use that same pair.

### Call

A message asking the other side to run one method. The guest issues one to reach a Receiver; the host issues one only to re-enter a Block.

### Reply

The answer to one Call: a value, a Fault, a break, or Unsupported. A break carries nothing; it continues the exit a Block took. The answer to a Yield may instead be the Error Record of a Block that raised.

```
Call  -> value | Fault | break | Unsupported
Yield -> value | break | Error Record | Unsupported
```

### Block

Guest code passed alongside a Call. It stays in the guest, and only its presence crosses the boundary.

### Yield

One synchronous round-trip from the host into a Block whose Call is still open.

### Proxy

The guest-side behavior that turns a method call on a bound constant or a Handle into a Call.

### Entrypoint

The constant a Snippet defines for a Run to call.

### Run

An Invocation that names an entrypoint already loaded in the guest, instead of supplying source to execute.

### Invocation

One run of guest code, from entry until it settles. It is the act; the Execution is the record it leaves.

### Execution

The record one Invocation leaves — what it produced, what it wrote, and what it consumed. Frozen once the Invocation settles, and never revised.

### Outcome

The final result of one Invocation. Every Invocation answers exactly one, whether it succeeded or failed.

## Host

### Includes

- `README.md`
- `docs/guides/**/*.md`
- `lib/**/*.rb`
- `crates/kobako/src/**/*.rs`
- `examples/**/*.rb`
- `examples/**/*.md`

### Pool

A bounded set of identically set-up Sandboxes, each handed to one holder at a time.

### Catalog

What a Sandbox has registered: the bindings, preloads, and Flags fixed at setup. It also holds the Handle table each Invocation mints for itself.

### Receiver

The host object a Call resolves to — a Service by its path, or the object a Handle names.

### Exposure

The methods a host object lets the guest call through a reference to it, by name or as a Handle. The object may declare its Exposure itself. Otherwise the Exposure derives from what the object and its own class define, fixed when the reference is made. It only narrows: nothing it permits reopens what the boundary refuses.

#### Rejected

- `allow-list` - Names the list rather than what it governs. The subset an object permits is its Exposure, not a second concept.

### Extension

A guest idiom paired with an optional Backend, installed as one unit so guest code sees a native-style constant.

### Backend

The host side of an Extension: the object bound at its path. It is fixed for the Sandbox, supplied afresh each Invocation, or left for the Host App to fill.

## Wire

### Includes

- `docs/wire/**/*.md`
- `crates/kobako-transport/**/*.rs`
- `crates/kobako-codec/**/*.rs`
- `crates/kobako/src/msgpack.rs`
- `crates/kobako/src/msgpack/**/*.rs`
- `lib/kobako/codec.rb`
- `lib/kobako/codec/**/*.rb`
- `lib/kobako/payload.rb`
- `lib/kobako/payload/**/*.rb`
- `wasm/kobako-core/**/*.rs`
- `wasm/kobako-mruby/src/msgpack.rs`
- `wasm/kobako-mruby/src/msgpack/**/*.rs`

### Wire Spec

The contract every message crossing the boundary answers to. Each side implements it independently, so it is an agreement rather than a shared component.

### Envelope

The part of a message that says where it goes and how it turned out. It is readable without a Codec, so routing and attribution never depend on one.

#### Rejected

- `core envelope` - a second name for the Envelope.

### Tag

The first byte of every message, naming its operation or its answer's arm. One registry covers both directions, so an assigned tag means one thing everywhere.

### Request

A message asking the receiver to run the operation its tag names. A request under a tag the registry has not assigned is unknown rather than broken.

### Downcall

A request from the host to the guest. It starts an Invocation or a Yield, or asks the guest to compile its Snippets.

### Upcall

A request from the guest to the host. It carries a Call.

### Codec

The agreement two endpoints reach about how values become bytes. It is replaceable: only the two endpoints need to share one.

#### Rejected

- `dialect` - a second name for the same agreement.

### Overlay

How one endpoint's Codec speaks to that endpoint's own objects, a Handle included. It lives where those objects live. The Filesystem's Overlay is another thing under the same word.

### Value Budget

The most values one guest payload may hold, worked out from the memory cap. Decoding spends it, so a short payload cannot make the host build a large value.

### Fault

The reason a Call is refused, returned to whoever issued it. It is kobako's own data rather than the caller's, so it rides the Envelope.

### Unsupported

The answer that the receiver does not serve the operation it was sent, returned to whoever sent it. It is an answer, not a breakage, so it is never a Trap.

### Error Record

A failure's name, message, and backtrace as one unit. Every answer that reports a guest failure carries this same shape.

### Panic

The failed arm of an Outcome. It attributes the failure to a side of the boundary and carries the Error Record describing it.

### Sandbox failure

A failed Invocation the guest's own code or the wire is answerable for.

### Service failure

A failed Invocation a Service is answerable for.

### Preamble

What the guest reads before an Invocation's entry runs or its preloads compile: the Catalog's bindings, preloads, and Flags. The guest starts knowing what this run was configured with.

```
Preamble = bindings + preloads + Flags
   read before -> an Invocation's entry
               -> a compile of the preloads
```

### Flag

A named switch the host sets for an Invocation's interpreter. A Guest Binary that does not recognise it skips it, unless the host marked it critical.

## Isolation

### Includes

- `crates/kobako-runtime/src/**/*.rs`
- `crates/kobako-wasmtime/src/**/*.rs`
- `ext/**/*.rs`
- `docs/isolation/**/*.md`

### Ambient

Authority the guest would hold without the Host App handing it over. The guest holds none of it unless a Profile grants it.

```
handed over by the Host App  -> Services, Handles, Grants
Ambient                      -> host time, entropy, files, environment, sockets
```

### Profile

The isolation a Wasm Driver builds around the guest, one of an ordered set. The Host App requests a profile as a floor, and the Wasm Driver declares the profile it built. Profiles differ only in which Ambient authority they grant.

```
permissive  <  hermetic
   requested profile  <=  declared profile   -> accepted
```

### Engine

The WebAssembly runtime that executes a Guest Binary, such as wasmtime. Only a Wasm Driver names it, so a Frontend works over any Engine.

### Wasm Driver

What drives one Engine, such as wasmtime, and encodes each ABI onto it. `kobako-runtime` beneath it decides each answer. The Wasm Driver keeps only the checks a syscall entry makes itself, such as which descriptor a call takes. Each implements `Runtime`, and wasmtime's is `kobako_wasmtime::Driver`.

```
guest --ABI--> Wasm Driver: decode, fd table, errno codes, entry checks --> runtime: the answer
```

### Grant

A host resource the Host App hands to the runtime instead of binding it by name. The guest reaches it through its system interface, such as a file it opens. The Sandbox holds it, and each Invocation receives it afresh.

```
Sandbox: Grant ──each Invocation──> runtime ──system interface──> guest
```

Each kind of Grant carries what it hands over and decides its own access. The runtime asks the Grant before the guest's call proceeds.

```
Grant ─┬─ Mount   -> a Filesystem at a guest path
       ├─ Network -> a named network entry, its endpoints and policies
       └─ ...     -> another kind, its own access
```

### Trap

A failed Invocation the engine is answerable for: a cap was reached, or no Outcome could be framed.

### Yielder

The host-side stand-in for a Block, valid only while the dispatch that received the Block lasts.

### Channel

One of the two streams guest code writes to: output and error. Each is captured apart, so what a script printed never mixes with what it warned.

## Filesystem

### Includes

- `crates/kobako-runtime/src/filesystem.rs`
- `crates/kobako-runtime/src/filesystem/**/*.rs`
- `crates/kobako-runtime/src/mount.rs`
- `crates/kobako-runtime/src/security.rs`
- `crates/kobako-runtime/src/vfs.rs`
- `crates/kobako-runtime/src/vfs/**/*.rs`
- `crates/kobako-wasmtime/src/preview1/**/*.rs`
- `ext/kobako/src/runtime/filesystem.rs`
- `ext/kobako/src/runtime/filesystem/**/*.rs`
- `lib/kobako/filesystem.rb`
- `docs/filesystem/**/*.md`

### Mount

The Grant placing a Filesystem at a guest path. It allows reading, or reading and writing, and a Policy may refuse more of that. The Mount decides, so the Filesystem behind it need not.

```
Mount -> at a guest path, read or read-write, or a Policy -> a Filesystem
```

### Policy

The Host App's decision on what a guest asks of a Grant. Each kind of request has its own hook, as an LSM hooks each kind of object. It is asked after the Grant itself allows the call, and can only refuse more.

A Mount's Policy judges each path opened or changed beneath it, as the Filesystem spells it. In Ruby it is the block a mount is given. A network entry's Policies judge each name looked up, each connection, and each HTTP request. In Ruby each is the entry's `resolve?`, `connect?`, or `request?`, or a `policy` block.

```
a path beneath a Mount  --+
a name, a connection,   --+--> Policy --+-- allows -> the call proceeds
or a request                            +-- refuses -> denied, the script goes on
```

### Filesystem

The files and directories behind a Mount, and the names reached in them. It answers every call the guest makes beneath that Mount's path, and every Invocation reaches the same one. Its Mounts share those names, as Linux's `super_block` does, so a name moved beneath one is moved beneath all. kobako ships three, and a Host App may bring its own or wrap another.

```
Bind     -> a host directory; nothing outside it is reachable
Tmpfs    -> held in memory; lives as long as the Host App's object
Overlay  -> Layers stacked into one view; changes land in its upper Tmpfs
```

#### Rejected

- `Filesystem Driver` - Linux's informal name; beside the Wasm Driver it reads as a second kind.

### Layer

One Filesystem stacked in an Overlay, as overlayfs stacks its layers. The lower Layers are read-only. The upper Layer, a Tmpfs, takes every change; without one the Overlay is read-only. A Layer hides the same path in the Layers below it, and a whiteout hides a removed one.

```
Overlay -> upper Layer (a Tmpfs, writable) -> lower Layer (read-only) -> ...
```

### Identity

The device and inode numbers that key an Inode, as `st_dev` and `st_ino` do. A directory keeps its identity when it moves, and a directory made in its place has another. A directory a Filesystem opens stays open, so its number stays its own while it is. An inode number of zero reports none.

```
open a directory beneath a Policy -> remember its identity
later call -> same identity at its path ─┬─ yes -> proceeds
                                          └─ no, or none -> ENOENT
```

### Inode

One entry of a Filesystem, as a lookup finds it: its kind, its Identity, and the operations answering for it. A directory's operations answer about one name in it at a time. A Filesystem is written as those operations, as Linux writes `inode_operations`. Whatever an entry lacks, the runtime answers for it.

### Cred

Who a Guest's calls are made for, as Linux's `struct cred`. It holds the security hooks one Invocation registered, so a Policy reaches the runtime the way an LSM does.

```
Invocation's Mounts -> Cred -> each call's security hooks
```

### Dentry

One name of a Filesystem and the directory above it, made by the runtime alone. It carries the name as the directory stores it, so every spelling of one entry reaches the same Dentry. A name moved through the runtime takes its Dentry along, and one removed through it is dead. A Filesystem reads the name a call is about from its Dentry.

The runtime records on the Dentry the Inode a lookup finds, as Linux's `d_add` does, and asks no more. A Filesystem may answer that a held Dentry no longer names what it recorded. The runtime then drops that Dentry for the next lookup, and its holders keep it alive.

### Open File

What a descriptor refers to, as Linux's `struct file`. It holds where it was opened, what answers for it, its position, and what it was opened for. `kobako-runtime` holds it, so every ABI shares one, and each ABI's descriptor table refers to it. Each standard stream is one too, an end of a pipe with no position.

### Path

An entry the guest reached beneath a Mount, as the Filesystem mounted there and the Dentry naming it. The runtime walks a guest path from a directory's, one name at a time.

```
Path = mounted Filesystem + Dentry ─▶ Inode ─▶ lookup ─▶ the next Path
```

## Network

### Includes

- `crates/kobako-runtime/src/network.rs`
- `crates/kobako-runtime/src/network/**/*.rs`
- `crates/kobako-runtime/src/net.rs`
- `crates/kobako-runtime/src/net/**/*.rs`
- `crates/kobako-runtime/src/security.rs`
- `crates/kobako-wasmtime/src/preview2/**/*.rs`
- `ext/kobako/src/runtime/network.rs`
- `lib/kobako/network.rb`
- `lib/kobako/catalog/network.rb`
- `docs/network/**/*.md`

### Entry

One named unit of the network a Host App hands a Sandbox, each one Grant. The Sandbox holds each name once, and a Context may replace one by its name for an invocation. An entry plays any of three roles: its endpoints, its credentials, and its policies.

```
sandbox.network(name, entry) -> one Grant -> endpoints, credentials, policies
```

### Endpoint

What one entry lets the guest reach: a host, a port, a protocol, and where its name may lead. It is data, so the runtime judges it on any thread. Unless written otherwise, an endpoint speaks HTTP and leads only to public addresses.

```
host:port, protocol (HTTP | TCP | UDP), addresses ([:public] unless written)
```

### Address Set

A named class of real addresses an endpoint may lead to. Three are an endpoint's to name, and two are fixed sets kobako refuses whatever an entry says.

```
public, local, private   - an endpoint names these
metadata, reserved       - fixed sets, always refused
```

### Ruleset

The rule data compiled each invocation from every entry's endpoints and the address sets. It judges what a name or an address the guest reaches may be.

```
endpoints of every entry + address sets -> Ruleset
```

### Credential

A secret an entry binds to its own endpoints. The guest holds only a placeholder, and the secret is swapped in as a request leaves over HTTPS.

```
guest: Authorization: Bearer <placeholder> -> wire: Bearer <secret>
```

### Protocol Family

The backend that reaches the host network, as Linux's `net_proto_family`. It dials the exact address the ruleset passed, and resolves the names a lookup asks.

```
Namespace -> Protocol Family -> host sockets, host resolver
```

### Outbound Seam

The replaceable sender of a guest's HTTP request, as wasmtime's `send_request`. It frames and sends what the ruleset already admitted.

```
L7 proxy -> outbound seam -> socket layer -> the server
```

### Namespace

One invocation's view of the network, as Linux's `struct net`. It holds the ruleset, the virtual addresses, the Protocol Family, and the connection limits. The socket layer and the L7 proxy read them; it judges nothing itself.

```
socket layer, L7 proxy -> read -> Namespace: ruleset, virtual addresses, limits
```

### Socket Hooks

The socket half of the security hooks, as Linux's `security_socket_*`. The modules a credential holds, then each entry's policy, judge what the guest named before the ruleset.

```
guest names an address -> socket hooks -> ruleset -> dial
```

### Virtual Address

The address a lookup hands the guest in place of one real address. No second name reuses it. A connect to it dials the one real address it stands for, so the guest never learns one.

```
lookup "api.example.com" -> virtual address -> connect -> the real addresses
```

### Connector

The dialer the runtime hands the outbound seam, so an HTTP request opens its connection through the Namespace. The seam is supplied one and never implements its own.

```
outbound seam -> connector -> Namespace -> Protocol Family
```

### Host-side Producer

A socket, a lookup, or an HTTP response the host runs for the guest. Each takes one descriptor, as `socket()` takes one fd, and its views take none.

```
producer: socket | lookup | response -> one descriptor -> its views share it
```

### Socket File

The Open File a socket stands behind, as Linux's `sock_alloc_file` makes one. It never waits, and only preview2 reaches it. The guest adapts it into an IO it reads, writes, and waits on.

```
socket -> socket file -> preview2 streams, pollables -> guest IO
```
