# Architecture

kobako is assembled from parts. This document maps what each part owns, what it
depends on, where each endpoint's dialect sits, and how the Ruby gem's `lib/` is
tiered. Which level a user stands on is in
[`guides/assembly-levels.md`](guides/assembly-levels.md).

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

| Part | Owns | Depends on |
|---|---|---|
| `kobako-transport` | the core envelope and the ABI's values | nothing, ever |
| `kobako-codec` | the payload dialects, one namespace and feature per schema | nothing |
| `kobako-runtime` | the engine contract: `Runtime`, `DispatchHandler`, `Yielder`, `Profile`, `Snapshot` | transport |
| `kobako-wasmtime` | one engine behind that contract | runtime, transport |
| `kobako` | the Rust host model: `Sandbox`, `Receiver`, `Handles`, `Execution` | transport, runtime, wasmtime *(optional)*, codec *(optional)* |
| `lib/` | the Ruby host model and its own dialect implementation | the native ext |
| `ext/` | the magnus surface, a byte shuttle between Ruby and the driver | runtime, transport, wasmtime |
| `kobako-core` | the guest ABI: the `Guest` trait, `export_guest!`, the dispatch proxy | transport |
| `kobako-mruby` | the mruby guest model: `MrbGuest` flows and the wire-tied bridge gem | core, transport, beni, codec *(optional)* |
| `kobako-io` · `-regexp` · `-json` | capability gems: guest-local behaviour, no wire | beni |
| `kobako-wasm` | the shipped shell, naming the schema and the gem set | all of the guest side |

### Dialect Overlays

An overlay is how one endpoint's dialect speaks to its own objects.
It decodes a payload into them, wraps one back out, and reaches a bound object through whatever seam that took.
Each of the three endpoints has one, living where that endpoint's objects live.

| Endpoint | Dialect implementation | Overlay |
|---|---|---|
| Ruby gem | `lib/kobako/{codec,payload}/`, an independent second implementation | the same files |
| Rust SDK | `kobako-codec` | `kobako`'s `msgpack` module |
| mruby guest | `kobako-codec` | `kobako-mruby`'s `msgpack` module |

### Overlay Placement

Where an overlay sits follows from where the endpoint's objects are.
That is why the two placements that look odd are correct.

| Case | Placement |
|---|---|
| shared dialect crate | the overlay is a module of the endpoint's own beside it |
| Ruby gem | the Handle walk in `lib/kobako/codec/` is the overlay, not a misplacement |
| `ext/` | no overlay: a shuttle holds no objects to bind a dialect to |
| a dialect kobako does not ship | wherever its objects live, even outside this repository |

The Ruby gem writes the dialect itself, so the dialect already sits where its objects are.
`ext/` has Ruby's values on one side and the driver's bytes on the other.

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
| guest crates link libmruby on every build | no code hides behind a linked-only `cfg` |
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
