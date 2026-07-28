# Architecture — which layer to stand on

kobako is assembled from parts, and how many of them you choose is up to you.
This document is the map: four levels, each widening what is yours and each
costing more to stand on, so you can find the one that varies what you need
varied and stop there.

Read this before [`customization.md`](customization.md). That document states
what each replaceable interface obliges its implementer to do; this one tells
you which interfaces you are even going to meet.

| Document | Answers |
|---|---|
| [`variants.md`](variants.md) | what we ship |
| **this one** | which level to stand on |
| [`customization.md`](customization.md) | what the interfaces at that level oblige you to |

## The ladder

```
     what you name                        ┌──────────────────────────┐
                                          │      the fixed pillar     │
 L1  gem "kobako"                         │                          │
     └ the bundled data/kobako.wasm       │   kobako-transport       │
                                          │   ├ the core envelope    │
 L2  gem "kobako" + a guest you build     │   └ the ABI's values     │
     └ kobako-mruby, or your own guest    │                          │
                                          │   depends on nothing;    │
 L3  the kobako crate (Rust host SDK)     │   every level depends    │
     └ any guest, any engine              │   on it                  │
                                          │                          │
 L4  kobako-transport + kobako-runtime    │   the same in every      │
     └ a frontend you write               │   assembly, which is     │
       (kobako's own Ruby gem is one)     │   what makes the parts   │
                                          │   interchangeable at all │
                                          └──────────────────────────┘
```

What is yours at each level:

| | payload schema | capability gems | invocation flows | guest language | wasm engine | host frontend |
|---|---|---|---|---|---|---|
| **L1** | MessagePack | `kobako-io` | the harness's | mruby | wasmtime | the Ruby gem |
| **L2** | MessagePack ⚠ | **yours** | **yours** | **yours** | wasmtime | the Ruby gem |
| **L3** | **yours** | **yours** | **yours** | **yours** | **yours** | the SDK's ⚠ |
| **L4** | **yours** | **yours** | **yours** | **yours** | **yours** | **yours** |

The ⚠ cells are the traps: the two places where the freedom looks available
from where you stand and is not. The other fixed cells surprise nobody — you
picked the Ruby gem, so you get its engine.

## L1 — the bundled assembly

`gem "kobako"` and the `data/kobako.wasm` it ships. Every choice is made for
you: MessagePack on the wire, `kobako-io` as the only capability, the hermetic
isolation floor, wasmtime as the engine, the mruby harness's own invocation
flows. Point a Sandbox at a downloadable variant to add Regexp or JSON
([`variants.md`](variants.md)) and you are still here — a variant is a
different artifact, not a different level.

## L2 — your own guest, the same wire

Build a Guest Binary of your own: a leaf shell over `kobako-mruby`, naming the
`beni::Gem` set your scripts may reach and, where the harness's flow is not
what you want, replacing one. Leaving mruby behind is this level too — the gem
points at whatever artifact you hand it, and an interpreter that satisfies the
ABI is one it can drive. Either way the Ruby gem drives it unchanged, because
the guest still speaks what the gem speaks.

**⚠ The schema is not yours here.** A guest shell names its codec on
`MrbGuest::Codec`, so the freedom looks available — but the Ruby frontend has
no matching seam. It speaks MessagePack directly, and a guest that answers in
anything else has nothing to answer to. Wanting your own schema means L3.

## L3 — your own host, in Rust

The `kobako` crate is a second frontend over the same driver, and at this level
every part below it is yours to pick: the payload schema (build without the
`msgpack` feature and each payload position is bytes you own), the guest (any
artifact satisfying the ABI, in any language), the engine (anything satisfying
the `kobako-runtime` contract, handed to `Sandbox::with_runtime`).

**⚠ The host model is the SDK's.** Services bound at constant paths, Handles
minted per invocation, Extensions composed over preload and bind, registration
sealed at the first invocation — that shape is what the SDK is. It is the same
shape the Ruby gem has, and the differential parity harness holds the two to
it. Wanting a different one means L4.

## L4 — your own frontend

Compose `kobako-transport` and `kobako-runtime` directly and write the host
model you want. This is not an exotic path: kobako's own Ruby gem is an L4
assembly, reaching the driver through a magnus shim rather than through the
Rust SDK, and so is any host in a language that is not Rust.

What you inherit at this level is the wire and the ABI. What you owe is
everything above them.

## The parts

`kobako-codec` is a **dialect** — MessagePack is the one we ship, and another
schema is another namespace beside it. `kobako-transport` is the **grammar**
both ends share whatever dialect fills a payload. Everything else is one
endpoint or another assembling those two into a model of its own.

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
| `kobako-codec` | the payload dialects — one namespace and one feature per schema | nothing |
| `kobako-runtime` | the engine contract: `Runtime`, `DispatchHandler`, `Yielder`, `Profile`, `Snapshot` | transport |
| `kobako-wasmtime` | one engine behind that contract | runtime, transport |
| `kobako` | the Rust host model: `Sandbox`, `Receiver`, `Handles`, `Execution` | transport, runtime, wasmtime, codec *(optional)* |
| `lib/` | the Ruby host model, and its own implementation of the dialect | the native ext |
| `ext/` | the magnus surface — a byte shuttle between Ruby and the driver | runtime, transport, wasmtime |
| `kobako-core` | the guest ABI: the `Guest` trait, `export_guest!`, the dispatch proxy | transport |
| `kobako-mruby` | the mruby guest model: the `MrbGuest` flows and the wire-tied bridge gem | core, transport, beni, codec *(optional)* |
| `kobako-io` · `-regexp` · `-json` | capability gems — guest-local behaviour, no wire | beni |
| `kobako-wasm` | the shipped shell: names the schema and the gem set | all of the guest side |

### Where a dialect meets objects

An **overlay** is one endpoint's answer to "how does this dialect speak to my
objects" — decoding a payload into them, wrapping one back out. There are three
endpoints, so there are three overlays, and each lives where that endpoint's
own objects live:

| Endpoint | dialect implementation | overlay |
|---|---|---|
| Ruby gem | `lib/kobako/codec/` — an independent second implementation | `lib/kobako/payload/`, the Handle walk |
| Rust SDK | `kobako-codec` | `kobako`'s `msgpack` module |
| mruby guest | `kobako-codec` | `kobako-mruby`'s `msgpack` module |

`ext/` has none, and that is not an omission: a shuttle has no objects of its
own to bind a dialect to — Ruby's values are on one side of it and the driver's
bytes on the other. The same reasoning places a dialect kobako does not ship:
its overlay belongs in the crate holding the objects it speaks to, which is why
one can be written entirely outside this repository.

## The fixed pillar

`kobako-transport` — the core envelope and the ABI's values — is the same at
every level and in every assembly. It depends on nothing, so taking it does not
mean taking anyone else's choices, and everything else depends on it, which is
why a host, a payload codec, and a guest can be chosen independently at all.

A payload rides inside that envelope untouched. That is the whole reason the
schema is replaceable: routing a message and attributing its outcome never read
a payload byte, so swapping the schema leaves the envelope, the ABI, and the
version alone (→ [`wire-codec.md`](wire-codec.md)).

## Where to go next

| You are heading for | Read |
|---|---|
| L1, with a capability the default lacks | [`variants.md`](variants.md) |
| L2 or beyond | [`customization.md`](customization.md) — the obligations each seam carries |
| the wire itself | [`wire-contract.md`](wire-contract.md), then [`wire-codec.md`](wire-codec.md) |
| what the sandbox does and does not defend | [`security-model.md`](security-model.md) |
