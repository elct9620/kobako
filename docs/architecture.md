# Architecture

This is kobako's one global map. It names each part, the seams between them, and the rules that hold across all of them. Every detail lives in a Context document this map links to.

| Question | Section |
|---|---|
| what kobako adds to an Engine | Bird's Eye |
| where kobako meets WASI, now and later | WASI Standards |
| which part owns this | Code Map |
| what every assembly shares | Fixed Pillar |
| where a replacement plugs in | Seams |
| what stays in the core | Thin Core |
| what a word means where | Contexts |
| what must never exist | Invariants |
| which reference decides a behavior | Baselines |
| what happens when an Engine cannot do something | Capability Negotiation |

Which level a user stands on is in [`guides/assembly-levels.md`](guides/assembly-levels.md).

## Bird's Eye

An Engine is the WebAssembly runtime that executes a Guest Binary, such as wasmtime. kobako builds only what an Engine's native WASI cannot give but kobako's guarantees need.

| kobako provides | Because |
|---|---|
| every preview1 call that reads the descriptor table: `fd_*`, `path_*`, `sock_*`, `poll_oneoff` | each Mount is a descriptor, and one table must answer every call that reads one |
| every `wasi:sockets` and `wasi:http` import, and the `wasi:io` they stand on | a socket must pass the endpoints a Grant names, which no Engine knows |
| the monotonic clock's `subscribe-duration` | its pollable waits in kobako's `wasi:io`, beside a socket's |
| the host side of the kobako ABI | no Engine knows that ABI |

Arguments, environment, reading a clock, process, scheduling, and randomness stay with the Engine's native WASI.

```
+-------------+
| host model  |
+------+------+
       | Runtime
       v
+-------------+  drives   +--------------+
| Wasm Driver |---------->| Engine       |
+--+-------+--+           +------+-------+
   |       ^                     | runs
   | asks  | imports             v
   |       |              +--------------+
   |       +--------------| Guest Binary |
   v                      +--------------+
+----------------+
| kobako-runtime |
+----------------+
```

- The host model is a Frontend and its Codec; it drives a Wasm Driver through the `Runtime` trait.
- The Wasm Driver encodes each ABI onto the Engine. It keeps only a syscall entry's own checks and the steps only its ABI has.
- `kobako-runtime` decides every other answer kobako provides.

## WASI Standards

kobako meets WASI at one layer and follows its versions by adding encodings. Why that layer, and when a version is taken up, are in [`isolation/README.md`](isolation/README.md).

| Question | Answer |
|---|---|
| where kobako replaces WASI | the core-module imports, for the calls that read the descriptor table |
| where kobako takes WASI 0.2 | `wasi:sockets` and `wasi:http`, imported by a core module, since preview1 has no connect |
| how a new WASI version joins | as its own encoding and numbered table; older ones adapt to it |
| what every version shares | the one `kobako-runtime`, which holds the Open File |

## Code Map

`kobako-transport` is the grammar both ends share. `kobako-codec` is a Codec beside it: MessagePack is the one kobako ships. Every other part is an endpoint assembling those two into a model of its own.

```
 HOST                             :  GUEST (wasm32)
                                  :
 +----------+                     :  +--------------+
 | lib/     |                     :  | kobako-wasm  |
 | Ruby gem |                     :  | shell        |
 +----+-----+                     :  +------+-------+
      |                           :         | builds on
 +----v-----+     +----------+    :  +------v-------+
 | ext/     |     | kobako   |    :  | kobako-mruby |
 | bridge   |     | Rust SDK |    :  | guest model  |
 +--+----+--+     +-+------+-+    :  +------+-------+
    |    |          .      |      :         | calls
    |  +-v----------v-+    |      :  +------v-------+
    |  | kobako-      |    |      :  | kobako-core  |
    |  | wasmtime     |    |      :  | guest ABI    |
    |  +------+-------+    |      :  +------+-------+
    |         | asks       |      :         |
 +--v---------v------------v--+   :         |
 | kobako-runtime             |   :         |
 +-------------+--------------+   :         |
               |                  :         |
               v                            v
 +----------------------------------------------+
 | kobako-transport                             |
 +----------------------------------------------+
```

- The dotted edge is optional: the Rust SDK builds without `kobako-wasmtime`.
- `lib/` reaches the rest only through `ext/`, the Ruby gem's native half and part of its host model.
- `kobako-codec` sits beside `kobako-transport`; the Rust SDK and `kobako-mruby` take it optionally, and the Ruby gem writes its own.
- The Guest Binary's variants are in [`guides/variants.md`](guides/variants.md).

### Part Ownership

Each part owns one concern. Its dependencies on other kobako parts may only point the way this table does.

| Part | Owns | kobako parts it uses |
|---|---|---|
| `kobako-transport` | the Envelope and the ABI's values | none, ever |
| `kobako-codec` | the payload Codecs, one namespace and feature per Codec | none |
| `kobako-runtime` | the contract: `Runtime`, `DispatchHandler`, `Yielder`, `Profile`, `Snapshot`, `Grant`, `Network` | transport |
| `kobako-runtime` | what every Wasm Driver keeps: `endpoint`, `vfs`, `security`, `Mount`, `sched`, `wait`, `memcg`, the Filesystems, `net` | transport |
| `kobako-wasmtime` | one Wasm Driver, `kobako_wasmtime::Driver`, and its preview1 and preview2 encodings | runtime, transport |
| `kobako` | the Rust SDK: `Sandbox`, `Receiver`, `Handles`, `Execution` | transport, runtime, wasmtime *(optional)*, codec *(optional)* |
| `lib/` | the Ruby gem and its own Codec | the native ext |
| `ext/` | the magnus bridge: Ruby values to Envelopes, objects to a Filesystem or network entry | runtime, transport, wasmtime |
| `kobako-core` | the guest ABI: `Guest`, `export_guest!`, the dispatch proxy | transport |
| `kobako-mruby` | the mruby guest model: `MrbGuest` flows and the bridge gem | core, transport, codec *(optional)* |
| `kobako-io` · `-regexp` · `-json` | capability gems: guest-local behavior, no wire | none |
| `kobako-wasm` | the shipped shell, naming the Codec and the gem set | core, mruby, io, regexp *(optional)*, json *(optional)* |
| `kobako-baker` | the build-time bake of the boot state into an artifact | none |
| `kobako-parity` | the Rust half of the parity harness, never published | kobako, codec, transport |

### Outside Crates

Two rules hold for outside crates across every part. Each other outside crate states its reason in the `Cargo.toml` that names it.

| Rule | Why |
|---|---|
| `kobako-transport` takes no dependency, third-party included | taking it means taking no one else's choices |
| `wasmtime-wasi` sits only under `kobako-wasmtime` | it answers the preview1 calls that read no descriptor |

### Workspaces and Shipping

Three Cargo workspaces keep the host and guest dependency graphs apart.

| Workspace | Members | Target |
|---|---|---|
| root `Cargo.toml` | `ext/kobako` | host |
| `crates/` | `kobako`, `kobako-codec`, `kobako-parity`, `kobako-runtime`, `kobako-transport`, `kobako-wasmtime` | host |
| `wasm/` | `kobako-core`, `kobako-io`, `kobako-json`, `kobako-mruby`, `kobako-regexp`, `kobako-wasm` | wasm32-wasip1 |

`kobako-baker` sits under `wasm/` in a workspace of its own, which keeps wizer and wasmtime out of the guest graph.

The Ruby gem ships `lib/`, `ext/`, the three crates `ext/` depends on, and `data/kobako.wasm`. It leaves out the `crates/` workspace manifest, so those crates resolve as standalone packages and inherit nothing from it.

## Fixed Pillar

`kobako-transport` is the same at every level and in every assembly. It depends on nothing, so taking it means taking no one else's choices. Everything depends on it, so a host, a Codec, and a guest can be chosen independently.

```
+- Envelope (kobako-transport) ------------------------+
| routing, outcome      +- payload -----------------+  |
| read here             | never read here           |  |
|                       +---------------------------+  |
+------------------------------------------------------+
```

A payload rides inside the Envelope untouched, which makes the Codec replaceable. Routing a message and attributing its outcome never read a payload byte. Swapping the Codec therefore leaves the Envelope, the ABI, and the version alone; the layers are in [`wire/README.md`](wire/README.md).

## Seams

An assembly composes replaceable parts around the fixed pillar. Each line names what can be swapped on its right-hand side; `kobako-runtime` is what every Wasm Driver keeps.

```
Sandbox        = host model . Wasm Driver . kobako-runtime . Guest Binary
Wasm Driver    = Engine + preview1, preview2 encodings   (kobako-wasmtime)
kobako-runtime = host ABI endpoint + VFS + socket layer + L7 proxy seam
Guest Binary   = guest ABI endpoint + guest model + gems (kobako-wasm)
host model     = Frontend + Codec                        (ext/ + lib/ | kobako)
guest model    = flows + Codec                           (kobako-mruby | own Guest)
message        = Envelope . payload                      (transport . codec)
```

Three boundaries separate them. The kobako ABI is what a Guest Binary is built against; the two seams are where a replacement plugs in.

```
        seam H             kobako ABI           seam G
          :                   :                   :
  host    :   host ABI        :      guest ABI    :   guest
  model <-:-> endpoint <------:------> endpoint <-:-> model
          :   (kobako-runtime):    (kobako-core)  :   (flows)
```

| Seam | Contract |
|---|---|
| seam H | `kobako-runtime` |
| seam G | `Guest` and `proxy::dispatch` |
| between the endpoints | `kobako-transport` |

A Wasm Driver replaces the Engine and its encoding of each ABI, and keeps `kobako-runtime`. A guest replaces only its model and keeps `kobako-core`.

### Seam Crossings

A seam carries typed Envelopes and payload bytes, never encoded Envelope bytes. A change to the wire therefore stays inside the endpoints and `kobako-transport`.

| Crosses a seam | As |
|---|---|
| Call, Run, Reply, Yield Reply, Compile Reply, Outcome, Preamble | a `kobako-transport` type, or its fields |
| a payload | bytes only a Codec reads |
| a result that does not frame | a `Trap` |
| a host resource the guest reaches through WASI | a `Grant`, such as a `Mount` |
| a path call the guest makes beneath a Mount | a `vfs` operation |
| a lookup, a connection, or a request the guest makes | a socket layer or L7 proxy call, past the entries' policies |
| how a connection or a request leaves the host | a `ProtocolFamily`, an `Outbound` |
| how a name the guest or a request names is answered | a `Resolver` |
| how a Frontend's thread waits on any lock or host-side event | a `Blocking`, registered once |
| a Frontend giving up on an Invocation its callback cannot answer | `DispatchHandler::abandoned` |
| an Engine's memory and exports | a `GuestInstance` |

## Thin Core

`kobako-runtime` is an assembly point, and it stays small. It holds only what every assembly shares; full features plug in at its seams. Each part sits in one class below, and a part kept in the core names the guarantee that needs it.

| Class | Holds | Example |
|---|---|---|
| standard | behavior to a reference | the VFS, the socket layer |
| primitive | a shared mechanism | the wait queue, poll |
| encoding | an ABI put onto an Engine | a Wasm Driver's preview1, preview2 |
| extension | a replaceable part, defaulting to a crate | a Filesystem, the L7 proxy |
| outside crate | a crate taken as it is | rustls, `wasmtime wizer` |
| own | what nothing existing covers | the kobako ABI, the mruby guest |

### Lightweight Crates

A crate an extension's default takes is lightweight, as mio is. Outside Crates, above, holds the rules every crate meets.

| Trait | mio's case |
|---|---|
| one primitive | poll alone |
| no runtime or policy | hyper takes only its `rt` traits |
| every platform | epoll, kqueue, IOCP |
| a measured cost | the ext shrank as it replaced ureq |

## Contexts

A Context is an area with its own vocabulary and rules. A name is judged inside its Context. One word may mean two things in two Contexts, as the Filesystem's Overlay and a Codec's overlay do. The glossary keeps one section per Context.

| Context | Owns | Lives in | Detail |
|---|---|---|---|
| Host | the Sandbox lifecycle, what the guest may call, how a failure reaches the Host App | `lib/`, `kobako` | the header comment of `lib/kobako.rb`, [`guides/security.md`](guides/security.md), [`process/parity.md`](process/parity.md) |
| Wire | the Envelope, the tag registry, the Codecs | `kobako-transport`, `kobako-codec`, `kobako-core`, both `msgpack` overlays, `lib/kobako/{codec,payload}/` | [`wire/`](wire/README.md) |
| Isolation | the Engine, the Wasm Driver, the Ambient authority a Profile grants | `kobako-runtime`, `kobako-wasmtime`, `ext/` | [`isolation/`](isolation/README.md) |
| Filesystem | Mounts, Policies, the VFS, the Filesystems | `kobako-runtime`'s `filesystem`, `vfs`, `security`, `mount`; the preview1 encoding; their bridges in `ext/`, `lib/` | [`filesystem/`](filesystem/README.md) |
| Network | the named entries, their endpoints, the sockets | `kobako-runtime`'s `network`, `net`; the preview2 encoding; its bridges in `ext/`, `lib/` | [`network/`](network/README.md) |

The guest has no Context of its own. Its words are either shared or the Wire's, and the capability gems speak CRuby's. The Ruby gem's tiers and placement rules are in the header comment of `lib/kobako.rb`.

### Context Relations

A word crosses a Context only at an interface, which translates it.

```
+- Everywhere -------------------------------------------+
|                                                        |
|  Host --Codec overlay--> Wire <--Codec overlay--       |
|   |                                          guest     |
|   | ext/                                               |
|   v                                                    |
|  Isolation --Grant--> Filesystem, Network              |
+--------------------------------------------------------+
```

| Between | Relation | Translated by |
|---|---|---|
| Host or guest, and Wire | each endpoint takes the Fixed Pillar unchanged | its Codec overlay |
| Host and Isolation | the Host drives the Wasm Driver | the `kobako-runtime` contract, through `ext/` for Ruby |
| Isolation and Filesystem | the Filesystem conforms to the Grant contract | `Grant`, `Mount` |
| Isolation and Network | the Network conforms to the Grant contract | `Grant`, `Network` |

A Frontend hands every Context's words to the Host App, and an ABI endpoint reads the Wire's for its side. A word used there stays in the Context that defines it. A new kind of Grant joins as a Context beside Filesystem, conforming to the same contract.

## Invariants

These rules are written as what never exists, so a single counterexample breaks one. A rule naming no part holds across every part and build; one naming a part holds within it.

| Never exists | Kept by |
|---|---|
| a dependency of `kobako-transport`, third-party included | its `Cargo.toml` |
| a kobako part depending against Part Ownership | each `Cargo.toml` |
| a Codec in the default graph of `kobako-transport`, `kobako-core`, `kobako-mruby` | `crates:graph`, `wasm:graph` |
| a Codec or an Engine `kobako` cannot build without | `crates:graph` |
| magnus beneath `kobako-wasmtime` | `crates:graph` |
| Envelope bytes read or written outside the two ABI endpoints | `kobako_runtime::endpoint`, `kobako-core` |
| a Frontend or guest model that encodes, decodes, allocates, or packs | the Seam Crossings |
| an answer the Wasm Driver decides, beyond its entry checks and ABI-only steps | `kobako-runtime` |
| a call that succeeds where POSIX refuses it | each divergence being a refusal |
| a document linked or cited from source code, comments and tool output included | each comment stating its own intent |
| a guest crate on `beni` that skips libmruby on a build | no linked-only `cfg` |
| a Codec namespace under the Ruby gem's `Kobako::Codec` | Ruby has MessagePack only |

What a successful stat reports, such as an inode number, follows the Profile. `kobako-wasmtime` links no magnus, so a span with the GVL released cannot reach a Ruby value.

## Baselines

Behavior lives in the spec under [`spec/`](spec/). Each layer is judged against its own reference, and this order settles a conflict between them. How to apply it while writing code is in [`process/convention.md`](process/convention.md).

```
 order  source                    what it may do
 1      security                  refuse more; never succeed differently
 2      the newest WASI contract  bound what crosses; older versions adapt to it
 3      each layer's reference    decide the answer within its own layer
 4      a scenario                state each difference 1 or 2 forces on 3
```

Security wins because the guest is untrusted, and a refusal is the only departure that stays safe. The WASI contract comes next, since the guest speaks nothing else.

### Layer References

Each row names the one reference a layer's spec and code are judged against.

| Layer | Reference | Detail |
|---|---|---|
| `vfs` | Linux's VFS, in behavior and logic | [`filesystem/README.md`](filesystem/README.md) |
| `net` | Linux's sockets down to `proto_ops`; a backend below; HTTP, an egress proxy | [`network/README.md`](network/README.md) |
| `filesystem` | the spec | [`spec/`](spec/) |
| WASI host layer | WIT; where it leaves room, wasmtime-wasi | WASI Host Layer, below |
| guest Ruby | CRuby 3.4 on Linux in behavior; mruby in wording | [`spec/behavior/network.md`](spec/behavior/network.md), [`spec/behavior/regexp.md`](spec/behavior/regexp.md) |
| the two Frontends | each other | [`process/parity.md`](process/parity.md) |
| names and wording | the convention | [`process/convention.md`](process/convention.md) |

Guest Ruby covers `kobako-io` and every capability gem. Linux means its latest stable release, never a release candidate, so a reference holds still while kobako is judged against it.

### WASI Host Layer

kobako implements the WASI host layer itself, over `kobako-runtime`, so each source below plays one part.

| Source | Part |
|---|---|
| WIT, then witx | the contract: the calls, types, and error codes that cross |
| wasmtime-wasi | the reference implementation, matched where the contract leaves room |
| wasi-testsuite | the acceptance check for a preview1 answer |
| Linux | the answer itself, decided inside `kobako-runtime` |

An error is decided inside by Linux, crosses as the error code the WIT maps it to, and must be one wasi-testsuite accepts at preview1. Where preview1 and preview2 both cover a call, preview1 adapts preview2's answer, and preview2 takes no shape for preview1 ([`filesystem/preview1.md`](filesystem/preview1.md)).

## Capability Negotiation

A Wasm Driver whose Engine cannot do something natively says so, and the Frontend refuses by the Host App's floor. kobako does not fill an Engine's native gap.

| Mechanism | Declares | On a mismatch |
|---|---|---|
| `Runtime::profile` | the Profile the Wasm Driver built | the Frontend refuses below the floor |
| `Runtime::supports` | each kind of Grant it presents | the Frontend refuses an unsupported Grant |

A Wasm Driver whose Engine cannot freeze the clock therefore declares permissive. A Flag is no negotiation: a Guest Binary skips one it does not know unless it is critical, as [`wire/README.md`](wire/README.md) grows it.