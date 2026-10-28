# Architecture

kobako is assembled from parts. This document is the baseline for where a change belongs. It names the part that owns it, what that part may depend on, and the Ruby tier it joins.

| Question | Section |
|---|---|
| which part owns this | The Parts |
| what every assembly shares | The Fixed Pillar |
| where a replacement plugs in | The Seams |
| what a word means where | The Contexts |
| what holds across builds | Build Rules |
| where a Ruby type goes | Ruby Tiers |

Which level a user stands on is in [`guides/assembly-levels.md`](guides/assembly-levels.md).

## The Parts

`kobako-codec` is a dialect: MessagePack is the one we ship, and another schema is another namespace beside it.
`kobako-transport` is the grammar both ends share, whatever dialect fills a payload.
Every other part is an endpoint assembling those two into a model of its own.

```
  HOST                                       │  GUEST (wasm32)
 ═══════════════════════════════════════════ ╪ ══════════════════════════════
                                             │
  Ruby gem              Rust SDK             │   mruby guest
  ┌────────────┐        ┌────────────┐       │   ┌──────────────────┐
  │ lib/       │        │ kobako     │       │   │ kobako-mruby     │
  │ ┌────────┐ │        │ ┌────────┐ │       │   │ ┌──────────────┐ │
  │ │overlay │ │        │ │overlay │ │       │   │ │   overlay    │ │
  │ │payload/│ │        │ │msgpack/│ │       │   │ │   msgpack/   │ │
  │ └────────┘ │        │ └───┬────┘ │       │   │ └──────┬───────┘ │
  │ ┌────────┐ │        └─────┼──────┘       │   └────────┼─────────┘
  │ │dialect │ │ its own      │              │            │
  │ │codec/  │ │ Ruby impl    └──────┐   ┌───┼────────────┘
  │ └────────┘ │                     ▼   ▼   │
  └─────┬──────┘            ┌────────────────────────────────┐
        │                   │ kobako-codec — the dialect     │
  ┌─────▼──────┐            │ one namespace per schema       │
  │ ext/       │            └────────────────────────────────┘
  │ bridge     │  no dialect: Ruby's values on one side of it,
  └─────┬──────┘  the driver's envelopes on the other
        │
        │      ┌─ the SDK drives this too, or an engine you bring
  ┌─────▼──────▼───────────┐                  ┌──────────────────┐
  │ kobako-wasmtime        │ one engine       │ kobako-core      │
  │ ────────────────────── │                  │ the guest ABI    │
  │ kobako-runtime         │ the contract     │                  │
  └───────────┬────────────┘                  └─────────┬────────┘
              │                               │         │
 ═════════════▼═══════════════════════════════╪═════════▼════════════════════
         kobako-transport — the grammar: the core envelope + the ABI's
         values. Depends on nothing; everything above depends on it.
```

### Part Ownership

Each part owns one concern, and its dependencies on other kobako parts may only point the way this table does.

| Part | Owns | kobako parts it uses |
|---|---|---|
| `kobako-transport` | the core envelope and the ABI's values | none, ever |
| `kobako-codec` | the payload dialects, one namespace and feature per schema | none |
| `kobako-runtime` | the engine contract: `Runtime`, `DispatchHandler`, `Yielder`, `Profile`, `Snapshot`, `Grant`, `Mount`, `vfs`, `Inode` and kinds | transport |
| `kobako-wasmtime` | one engine behind that contract | runtime, transport |
| `kobako` | the Rust host model: `Sandbox`, `Receiver`, `Handles`, `Execution` | transport, runtime, wasmtime *(optional)*, codec *(optional)* |
| `lib/` | the Ruby host model and its own dialect | the native ext |
| `ext/` | the magnus bridge from Ruby values to the driver's envelopes and an `Inode` | runtime, transport, wasmtime |
| `kobako-core` | the guest ABI: `Guest`, `export_guest!`, the dispatch proxy | transport |
| `kobako-mruby` | the mruby guest model: `MrbGuest` flows and the bridge gem | core, transport, codec *(optional)* |
| `kobako-io` · `-regexp` · `-json` | capability gems: guest-local behaviour, no wire | none |
| `kobako-wasm` | the shipped shell, naming the schema and the gem set | every guest part |
| `kobako-baker` | the build-time bake of the boot state into an artifact | none |
| `kobako-parity` | the Rust half of the parity harness, never published | kobako, codec |

`kobako-transport` takes no dependency at all, third-party included. Elsewhere, `rmp` sits behind `kobako-codec`'s `msgpack` feature, `beni` under every guest crate that touches mruby, and the `msgpack` gem under `lib/`. `cap-std` sits under `kobako-runtime`'s Bind, and `wasmtime-wasi` under `kobako-wasmtime` for the preview1 calls kobako does not answer itself. `cap-fs-ext` opens a Bind's path for a Policy without following a link. `rustix` and `libc` ask the host what `cap-std` cannot.

### Dialect Overlays

An overlay is how one endpoint's dialect speaks to its own objects. It decodes a payload into them, wraps one back out, and reaches a bound object. It lives where that endpoint's objects live.

| Endpoint | Dialect implementation | Overlay |
|---|---|---|
| Ruby gem | `lib/kobako/{codec,payload}/`, a second implementation | the same files |
| Rust SDK | `kobako-codec` | `kobako`'s `msgpack` module |
| mruby guest | `kobako-codec` | `kobako-mruby`'s `msgpack` module |
| `ext/` | none | none |

The Ruby gem writes its dialect itself, so the Handle walk in `lib/kobako/codec/` is its overlay. `ext/` holds no objects to bind a dialect to. A dialect kobako does not ship places its overlay wherever its objects live, even outside this repository.

## The Fixed Pillar

`kobako-transport` is the same at every level and in every assembly.
It depends on nothing, so taking it means taking no one else's choices.
Everything depends on it, which is why a host, a payload codec, and a guest can be chosen independently.

```
 ┌─ core envelope (kobako-transport) ──────────────────────┐
 │ routing, outcome attribution    ┌─ payload ───────────┐ │
 │ read here                       │ never read here     │ │
 │                                 └─────────────────────┘ │
 └─────────────────────────────────────────────────────────┘
```

A payload rides inside that envelope untouched, and that is what makes the schema replaceable.
Routing a message and attributing its outcome never read a payload byte.
Swapping the schema therefore leaves the envelope, the ABI, and the version alone (→ [`wire/README.md`](wire/README.md)).

## The Seams

An assembly composes replaceable parts around the fixed pillar.
Each line names what can be swapped on its right-hand side.

```
Sandbox      = host model . engine . Guest Binary
engine       = engine mechanics + host ABI endpoint     (kobako-wasmtime)
Guest Binary = guest ABI endpoint + guest model + gems  (kobako-wasm)
host model   = frontend + codec                         (ext/ + lib/ | kobako)
guest model  = flows + codec                            (kobako-mruby | own Guest)
message      = envelope . payload                       (transport . codec)
```

Three boundaries separate them.
The wasm ABI is what a Guest Binary is built against; the two seams are where a replacement plugs in.

```
        seam H                 wasm ABI                 seam G
          :                       :                       :
  host    :    host ABI           :        guest ABI      :    guest
  model <-:->  endpoint  <--------:-------->  endpoint <--:->  model
          :    (driver)           :     (kobako-core)     :    (flows)
          :         \______ kobako-transport ______/      :
  contract: kobako-runtime            contract: Guest + proxy::dispatch
```

### Seam Rule

A seam carries typed envelopes and payload bytes, so only the two ABI endpoints read or write an envelope's bytes.
A frontend or a guest model never encodes, decodes, allocates, or packs.
A change to the wire therefore stays inside the endpoints and `kobako-transport`.

| Crosses a seam | As |
|---|---|
| Call, Run, Reply, Yield Reply, Compile Reply, Outcome, Preamble | a `kobako-transport` type, or its fields |
| a payload | bytes only a codec reads |
| a result that does not frame | a `Trap` |
| a host resource the guest reaches through WASI | a `kobako-runtime` `Grant`, such as a `Mount` |
| a path call the guest makes beneath a Mount | a `kobako-runtime` `vfs` operation |
| how a frontend's thread waits on a directory's or a copy up's lock | a `kobako-runtime` `Blocking` |
| a frontend giving up on an invocation its callback cannot answer | `DispatchHandler::abandoned` |

The two sides replace different units.
The host ABI endpoint shares `kobako-wasmtime` with the engine mechanics, so an engine is replaced whole.
A guest replaces only its model and keeps `kobako-core`.

## The Contexts

A context is an area with its own vocabulary and rules.
A name is judged inside its context, so the same word in two contexts may mean two things, as Overlay and a dialect overlay do.
The glossary keeps one section per context, scoped by its includes.

```
 ┌─ Everywhere: the terms every context shares ──────────────────┐
 │                                                                │
 │  Host ──dialect overlay──► Wire ◄──dialect overlay── guest     │
 │   │                                                            │
 │   │ ext/                                                       │
 │   ▼                                                            │
 │  Isolation ──Grant contract──► Filesystem                      │
 └────────────────────────────────────────────────────────────────┘
```

### Context Ownership

Each context owns the rules below and the parts that keep them.

| Context | Owns | Lives in |
|---|---|---|
| Host | the Sandbox lifecycle, what the guest may call, how a failure reaches the Host App | `lib/`, `kobako` |
| Wire | the envelope, the tag registry, the codecs | `kobako-transport`, `kobako-codec`, `lib/kobako/{codec,payload}/`, `docs/wire/` |
| Isolation | the engine and the Ambient authority a Posture grants | `kobako-runtime`, `kobako-wasmtime`, `ext/` |
| Filesystem | Mounts, Policies, the VFS, and the Filesystem kinds | `kobako-runtime`'s filesystem and vfs, the preview1 driver, their bridges in `ext/` and `lib/` |

The guest has no context of its own. Its words are either shared or the Wire's, and the capability gems speak CRuby's, which [`parity.md`](parity.md) governs.

### Context Relations

A word crosses a context only at an interface, which translates it.

| Between | Relation | Translated by |
|---|---|---|
| Host or guest, and Wire | each endpoint takes the Fixed Pillar unchanged | its dialect overlay |
| Host and Isolation | the Host drives the engine | the `kobako-runtime` contract, through `ext/` for Ruby |
| Isolation and Filesystem | the Filesystem conforms to the Grant contract | `Grant`, `Mount` |

A Frontend hands every context's words to the Host App, and an ABI endpoint reads the Wire's on behalf of its side. A word used there therefore stays in the context that defines it.

A new kind of Grant joins as a context beside Filesystem. It conforms to the same contract and brings its own words.

## Build Rules

Two structural facts no part's own code states.

| Rule | Why |
|---|---|
| a guest crate depending on `beni` links libmruby on every build | no code hides behind a linked-only `cfg` |
| `Kobako::Codec` has no schema namespace | Ruby is fixed to MessagePack and has no seam |

## Ruby Tiers

Inside `lib/`, a tier may use the tiers below it and never one above.

```
Orchestration   Sandbox, Pool, Runtime (+ ext), Context
      │
Catalog         setup-time registries + the per-invocation Handle table
      │
Transport ──┐   call value objects + dispatch
Outcome ────┤   guest-result attribution
      │     │
Payload ◄───┤   the [args, kwargs] shape a Call or Run carries
      │     │
Codec ◄─────┘   byte-level payload wire
      │
Root            dependency-free value objects and error classes
```

The core envelope has no tier here, because the native side frames and decodes it.

### Placement Rules

These rules keep the tiers acyclic; most were learned from a cycle or a leak.

| Rule | Example |
|---|---|
| a type sits at the lowest tier that needs it | `Kobako::Handle` at the root, for `Codec` |
| a native-defined handle that needs nothing in `lib/` sits at the root | `Kobako::Filesystem`, for `Catalog` |
| `Outcome` may require `transport/error.rb` | the gem contract fixes the class name |
| `Codec.track_handles` wraps only the decode call | wider leaks its flag into re-entry |

Namespace follows dependency direction, not which tier reads a type most. Do not move `Kobako::Transport::Error` to remove the lateral edge; its file depends only on root `errors.rb`.
