# Architecture

kobako is assembled from parts. This document is the baseline for where a change belongs. It names the part that owns it, what that part may depend on, and the Ruby tier it joins.

| Question | Section |
|---|---|
| which part owns this | The Parts |
| what every assembly shares | The Fixed Pillar |
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
  │ shuttle    │  no dialect: Ruby's values on one side of it,
  └─────┬──────┘  the driver's bytes on the other
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
| `kobako-runtime` | the engine contract: `Runtime`, `DispatchHandler`, `Yielder`, `Profile`, `Snapshot` | transport |
| `kobako-wasmtime` | one engine behind that contract | runtime, transport |
| `kobako` | the Rust host model: `Sandbox`, `Receiver`, `Handles`, `Execution` | transport, runtime, wasmtime *(optional)*, codec *(optional)* |
| `lib/` | the Ruby host model and its own dialect | the native ext |
| `ext/` | the magnus byte shuttle between Ruby and the driver | runtime, transport, wasmtime |
| `kobako-core` | the guest ABI: `Guest`, `export_guest!`, the dispatch proxy | transport |
| `kobako-mruby` | the mruby guest model: `MrbGuest` flows and the bridge gem | core, transport, codec *(optional)* |
| `kobako-io` · `-regexp` · `-json` | capability gems: guest-local behaviour, no wire | none |
| `kobako-wasm` | the shipped shell, naming the schema and the gem set | every guest part |
| `kobako-baker` | the build-time bake of the boot state into an artifact | none |
| `kobako-parity` | the Rust half of the parity harness, never published | kobako, codec |

`kobako-transport` takes no dependency at all, third-party included. Elsewhere, `rmp` sits behind `kobako-codec`'s `msgpack` feature, `beni` under every guest crate that touches mruby, and the `msgpack` gem under `lib/`.

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

These three rules keep the tiers acyclic; each was learned from a cycle or a leak.

| Rule | Example |
|---|---|
| a type sits at the lowest tier that needs it | `Kobako::Handle` at the root, for `Codec` |
| `Outcome` may require `transport/error.rb` | the gem contract fixes the class name |
| `Codec.track_handles` wraps only the decode call | wider leaks its flag into re-entry |

Namespace follows dependency direction, not which tier reads a type most. Do not move `Kobako::Transport::Error` to remove the lateral edge; its file depends only on root `errors.rb`.
