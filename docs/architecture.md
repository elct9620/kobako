# Architecture

This is kobako's one global map. It names each part, the seams between them, and the rules that hold across all of them. Every detail lives in a Context document this map links to.

| Question | Section |
|---|---|
| what kobako adds to an Engine | Bird's Eye |
| where kobako sits among WASI's layers | WASI Position |
| which part owns this | Code Map |
| what every assembly shares | Fixed Pillar |
| where a replacement plugs in | Seams |
| what a word means where | Contexts |
| what must never exist | Invariants |
| which reference decides a behavior | Baselines |
| what happens when an Engine cannot do something | Capability Negotiation |
| how a new WASI version joins | Standard Evolution |

Which level a user stands on is in [`guides/assembly-levels.md`](guides/assembly-levels.md).

## Bird's Eye

kobako builds only what an Engine's native WASI cannot give but kobako's guarantees need. The rest stays with the Engine.

| kobako provides | Because |
|---|---|
| the filesystem's answers | wasmtime-wasi keeps its descriptor table to itself |
| the host side of the kobako ABI | no Engine knows that ABI |
| the remaining WASI calls | not provided; native WASI answers them |

```
+-------------+
| Host model  |
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
+-------------+
| runtime     |
+-------------+
```

- The Wasm Driver catches every import the Guest Binary makes and encodes it onto the Engine.
- The runtime, `kobako-runtime`, decides every answer the guest sees.

## WASI Position

WASI can be replaced at eight layers. kobako replaces the core-module imports, layer 5, for the filesystem.

```
+---------------------------+
| guest side      L1-L4     |
+---------------------------+
| imports         L5        | <-- kobako
+---------------------------+
| WIT             L6        |
+---------------------------+
| host side       L7-L8     |
+---------------------------+
```

Layers 1 to 4 run in the guest's trust domain, so they cannot hold a boundary. Layer 6 needs the guest to be a component, and layer 7 has no hook left in wasmtime. The comparison is in [`isolation/README.md`](isolation/README.md).

## Code Map

`kobako-transport` is the grammar both ends share. `kobako-codec` is a dialect beside it: MessagePack is the one kobako ships. Every other part is an endpoint assembling those two into a model of its own.

```
 HOST                            :  GUEST (wasm32)
                                 :
 +------------+  +------------+  :  +--------------+
 | lib/ ext/  |  | kobako     |  :  | kobako-wasm  |
 | Ruby gem   |  | Rust SDK   |  :  | shell        |
 +-----+------+  +-----+------+  :  +------+-------+
       |  drives       |         :         | builds on
 +-----v---------------v------+  :  +------v-------+
 | kobako-wasmtime            |  :  | kobako-mruby |
 | Wasm Driver                |  :  | guest model  |
 +-------------+--------------+  :  +------+-------+
               | asks            :         | calls
 +-------------v--------------+  :  +------v-------+
 | kobako-runtime             |  :  | kobako-core  |
 | the runtime                |  :  | guest ABI    |
 +-------------+--------------+  :  +------+-------+
               |                 :         |
               v                           v
 +-------------------------------------------------+
 | kobako-transport                                |
 +-------------------------------------------------+
```

- `kobako-codec` sits beside `kobako-transport`; the Rust SDK and `kobako-mruby` take it optionally, and the Ruby gem writes its own.
- The capability gems, `kobako-baker`, and `kobako-parity` are in the table below.

### Part Ownership

Each part owns one concern. Its dependencies on other kobako parts may only point the way this table does.

| Part | Owns | kobako parts it uses |
|---|---|---|
| `kobako-transport` | the Envelope and the ABI's values | none, ever |
| `kobako-codec` | the payload dialects, one namespace and feature per Codec | none |
| `kobako-runtime` | the contract: `Runtime`, `DispatchHandler`, `Yielder`, `Profile`, `Snapshot`, `Grant` | transport |
| `kobako-runtime` | what every Wasm Driver keeps: `endpoint`, `vfs`, `security`, `Mount`, `sched`, the Filesystems | transport |
| `kobako-wasmtime` | one Wasm Driver, `kobako_wasmtime::Driver`, and its preview1 encoding | runtime, transport |
| `kobako` | the Rust SDK: `Sandbox`, `Receiver`, `Handles`, `Execution` | transport, runtime, wasmtime *(optional)*, codec *(optional)* |
| `lib/` | the Ruby gem and its own dialect | the native ext |
| `ext/` | the magnus bridge: Ruby values to Envelopes, a Ruby object to a Filesystem | runtime, transport, wasmtime |
| `kobako-core` | the guest ABI: `Guest`, `export_guest!`, the dispatch proxy | transport |
| `kobako-mruby` | the mruby guest model: `MrbGuest` flows and the bridge gem | core, transport, codec *(optional)* |
| `kobako-io` · `-regexp` · `-json` | capability gems: guest-local behavior, no wire | none |
| `kobako-wasm` | the shipped shell, naming the Codec and the gem set | core, mruby, io, regexp *(optional)*, json *(optional)* |
| `kobako-baker` | the build-time bake of the boot state into an artifact | none |
| `kobako-parity` | the Rust half of the parity harness, never published | kobako, codec, transport |

### Outside Crates

`kobako-transport` takes no dependency at all, third-party included. Every other outside crate sits under one part, for one reason.

| Crate | Under | For |
|---|---|---|
| `rmp` | `kobako-codec`'s `msgpack` feature | MessagePack bytes |
| `msgpack` gem | `lib/` | the Ruby gem's dialect |
| `beni` | every guest crate that touches mruby | the mruby wrapper |
| `cap-std`, `cap-fs-ext` | `kobako-runtime`'s Bind | confinement alike on every host, Windows included |
| `rustix`, `libc` | `kobako-runtime` | what `cap-std` cannot ask the host |
| `wasmtime-wasi` | `kobako-wasmtime` | the preview1 calls kobako does not answer |

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

An assembly composes replaceable parts around the fixed pillar. Each line names what can be swapped on its right-hand side; the runtime is what every Wasm Driver keeps.

```
Sandbox      = host model . Wasm Driver . runtime . Guest Binary
Wasm Driver  = Engine + preview1 encoding              (kobako-wasmtime)
runtime      = host ABI endpoint + VFS                 (kobako-runtime)
Guest Binary = guest ABI endpoint + guest model + gems (kobako-wasm)
host model   = Frontend + Codec                        (ext/ + lib/ | kobako)
guest model  = flows + Codec                           (kobako-mruby | own Guest)
message      = Envelope . payload                      (transport . codec)
```

Three boundaries separate them. The wasm ABI is what a Guest Binary is built against; the two seams are where a replacement plugs in.

```
        seam H             wasm ABI             seam G
          :                   :                   :
  host    :   host ABI        :      guest ABI    :   guest
  model <-:-> endpoint <------:------> endpoint <-:-> model
          :   (runtime)       :    (kobako-core)  :   (flows)
```

| Seam | Contract |
|---|---|
| seam H | `kobako-runtime` |
| seam G | `Guest` and `proxy::dispatch` |
| between the endpoints | `kobako-transport` |

A Wasm Driver replaces the Engine and its encoding of each ABI, and keeps the runtime. A guest replaces only its model and keeps `kobako-core`.

### Seam Crossings

A seam carries typed Envelopes and payload bytes, never encoded Envelope bytes. A change to the wire therefore stays inside the endpoints and `kobako-transport`.

| Crosses a seam | As |
|---|---|
| Call, Run, Reply, Yield Reply, Compile Reply, Outcome, Preamble | a `kobako-transport` type, or its fields |
| a payload | bytes only a Codec reads |
| a result that does not frame | a `Trap` |
| a host resource the guest reaches through WASI | a `Grant`, such as a `Mount` |
| a path call the guest makes beneath a Mount | a `vfs` operation |
| how a Frontend's thread waits on any lock | a `Blocking`, registered once |
| a Frontend giving up on an Invocation its callback cannot answer | `DispatchHandler::abandoned` |
| an Engine's memory and exports | a `GuestInstance` |

## Contexts

A Context is an area with its own vocabulary and rules. A name is judged inside its Context. One word may mean two things in two Contexts, as Overlay the Filesystem and a dialect overlay do. The glossary keeps one section per Context.

| Context | Owns | Lives in | Detail |
|---|---|---|---|
| Host | the Sandbox lifecycle, what the guest may call, how a failure reaches the Host App | `lib/`, `kobako` | `lib/` RDoc, [`process/parity.md`](process/parity.md) |
| Wire | the Envelope, the tag registry, the Codecs | `kobako-transport`, `kobako-codec`, `kobako-core`, both `msgpack` overlays, `lib/kobako/{codec,payload}/` | [`wire/`](wire/README.md) |
| Isolation | the Engine, the Wasm Driver, the Ambient authority a Posture grants | `kobako-runtime`, `kobako-wasmtime`, `ext/` | [`isolation/`](isolation/README.md) |
| Filesystem | Mounts, Policies, the VFS, the Filesystems | `kobako-runtime`'s `filesystem`, `vfs`, `security`, `mount`; the preview1 encoding; their bridges in `ext/`, `lib/` | [`filesystem/`](filesystem/README.md) |

The guest has no Context of its own. Its words are either shared or the Wire's, and the capability gems speak CRuby's. The Ruby gem's tiers and placement rules are in the RDoc of `lib/kobako.rb`.

### Context Relations

A word crosses a Context only at an interface, which translates it.

```
+- Everywhere -------------------------------------------+
|                                                        |
|  Host --dialect overlay--> Wire <--dialect overlay--   |
|   |                                          guest     |
|   | ext/                                               |
|   v                                                    |
|  Isolation --Grant--> Filesystem                       |
+--------------------------------------------------------+
```

| Between | Relation | Translated by |
|---|---|---|
| Host or guest, and Wire | each endpoint takes the Fixed Pillar unchanged | its dialect overlay |
| Host and Isolation | the Host drives the Wasm Driver | the `kobako-runtime` contract, through `ext/` for Ruby |
| Isolation and Filesystem | the Filesystem conforms to the Grant contract | `Grant`, `Mount` |

A Frontend hands every Context's words to the Host App, and an ABI endpoint reads the Wire's for its side. A word used there stays in the Context that defines it. A new kind of Grant joins as a Context beside Filesystem, conforming to the same contract.

## Invariants

These rules are written as what never exists, so a single counterexample breaks one. Each holds across every part and build.

| Never exists | Kept by |
|---|---|
| a dependency of `kobako-transport`, third-party included | its `Cargo.toml` |
| a kobako part depending against Part Ownership | each `Cargo.toml` |
| a Codec in the default graph of `kobako-transport`, `kobako-core`, `kobako-mruby` | `crates:graph`, `wasm:graph` |
| a Codec or an Engine `kobako` cannot build without | `crates:graph` |
| magnus beneath `kobako-wasmtime` | `crates:graph` |
| Envelope bytes read or written outside the two ABI endpoints | `kobako_runtime::endpoint`, `kobako-core` |
| a Frontend or guest model that encodes, decodes, allocates, or packs | the Seam Crossings |
| an answer the Wasm Driver decides | the runtime |
| a guest crate on `beni` that skips libmruby on a build | no linked-only `cfg` |
| a Codec namespace under `Kobako::Codec` | Ruby has MessagePack only |
| a divergence from POSIX that answers differently | it only refuses more |

`kobako-wasmtime` links no magnus, so a span with the GVL released cannot reach a Ruby value.

## Baselines

Behavior lives in the spec under [`spec/`](spec/). Each layer has one reference its spec and code are judged against.

| Layer | Baseline | Detail |
|---|---|---|
| WASI encoding | witx, then wasi-testsuite, then Linux | [`filesystem/preview1.md`](filesystem/preview1.md) |
| `vfs` | Linux's VFS, in behavior and logic | [`filesystem/README.md`](filesystem/README.md) |
| `filesystem` | the spec | [`spec/`](spec/) |
| capability gems | CRuby, except where security forbids | [`guides/json.md`](guides/json.md), [`guides/regexp.md`](guides/regexp.md) |
| the two Frontends | each other | [`process/parity.md`](process/parity.md) |
| names | the naming baseline | [`process/convention.md`](process/convention.md) |

wasmtime-wasi is a reference for the WASI encoding, never a baseline.

## Capability Negotiation

An Engine that cannot do something natively says so, and the Frontend refuses by the Host App's floor. kobako does not fill an Engine's native gap.

| Mechanism | Declares | On a mismatch |
|---|---|---|
| `Runtime::profile` | the Posture the runtime built | the Frontend refuses below the floor |
| `Runtime::supports` | each kind of Grant it presents | the Frontend refuses an unsupported Grant |
| Flag | a switch for the interpreter | a Guest Binary skips one it does not know, unless critical |

An Engine that cannot freeze the clock therefore declares permissive.

## Standard Evolution

Each ABI version gets its own call table, and every table asks the same runtime. A new WASI version adds an entry, not a rewrite.

```
 p1 module         p2 component
     |                  |
     v                  v
+-----------+   +-----------------+
| p1 table  |   | wasi:filesystem |
| now       |   | later           |
+-----+-----+   +--------+--------+
      |                  |
      +--------+---------+
               | asks
               v
      +-----------------+
      | kobako-runtime  |
      +-----------------+
```

The descriptor table belongs to each ABI's encoding, and p2 uses resources instead, so the tables do not share it. When a later version is taken up is in [`isolation/README.md`](isolation/README.md).
