# Customization — the interfaces a third party replaces

kobako is assembled from parts, and each assembly point is a named interface
someone outside this repository can implement. This document states what each
one obliges its implementer to do, and what kobako promises in return.

The boundary with [`variants.md`](variants.md): variants are the prebuilt
artifacts **we ship** — a matrix of capability gems composed onto one base.
Customization is the interfaces **a third party replaces**. A variant is
something to download; a customization point is something to implement.

Public names declared here are graded commitments, not guidance (N-9): a name
listed as stable does not change without an ABI version increment, and a
listed obligation is what an implementation is held to.

## What is replaceable

| Point | Interface | Endpoint |
|---|---|---|
| Payload codec | `MrbGuest::Codec: PayloadCodec` | guest |
| Payload codec | the byte-level payload surface, `msgpack` feature off | Rust host SDK |
| Capability set | `MrbGuest::init_gems` | guest |
| Invocation flow | a `Guest` method implemented rather than forwarded | guest |
| The whole guest | `impl kobako_core::Guest` + `export_guest!` | guest |
| Wasm engine | `impl Runtime`, handed to `Sandbox::with_runtime` | Rust host SDK |
| Wasm engine | `impl Runtime` + `DispatchHandler` + `Yielder` | a host frontend of your own |

Two things stay fixed. The **core envelope** and the **ABI surface** are the
same for every assembly — that is what makes the parts interchangeable at all
(→ [`wire-codec.md`](wire-codec.md)). Both live in `kobako-transport`, which
every tier above depends on and which depends on nothing, so an implementer
picks up the fixed tier without picking up anyone else's choices. And the
**Ruby frontend is fixed to the default codec**: MessagePack is Ruby's native
choice and the gem speaks it directly, so there is no seam to substitute at. A
replacement codec is named on the guest's `MrbGuest::Codec`; a Rust host takes
one by building the SDK without its `msgpack` feature.

## Payload codec

A codec owns the payload positions and nothing else. The core envelope routes
and attributes without reading a payload byte, so replacing the codec leaves
the envelope, the ABI, and the version untouched.

The two endpoints carry equal shares of that. A guest fills every position
below; a Rust host built on the SDK fills the same set, because each one has a
byte-level entry there — a dispatch's arguments and answer at `Receiver`, a
`run` payload at `RunPayload::bytes` or `RunPayload::build`, a yield at
`Yielder::call_payload`, an invocation's result at `Execution::payload`, and
the host object a Handle stands for at `Handles::resolve` during a dispatch or
`Execution::resolve` afterwards — both taking the id, since spelling a Handle
is the schema's job and reaching the object is not. Neither endpoint owes
anything at a Reply's fault arm, which rides the envelope.

That completeness is what lets a schema kobako does not ship reach the same
ergonomics without the SDK changing: each `msgpack`-gated member is a thin
wrapper over its byte-level entry, so another schema's overlay is written
outside this crate as extension traits over the same entries.

**Obligations** (→ [`wire-codec.md`](wire-codec.md) § What a replacement codec
must provide):

| Position | What the codec must express |
|---|---|
| Call and Run payload | positional and keyword arguments, distinguishably |
| Yield Reply ok / break body, Outcome result body | one value |

A Handle representation is optional. Without one, Handles ride only the
envelope's `target` field — a guest still reaches a stateful receiver and only
forgoes passing Handles as arguments.

A Reply's fault body is not a codec position. A Fault is kobako's own data — a
closed category and a message — so it rides the envelope, and a replacement
schema neither encodes nor reads one. A guest speaking another schema still
reads a refusal, and reads it with no codec at all.

**Where the choice is named.** A guest shell names it once, on the associated
type:

```rust
impl MrbGuest for MyGuest {
    type Codec = MyCodec;              // implements PayloadCodec
    fn init_gems(mrb: &Mrb) -> Result<(), beni::Error> { Ok(()) }
}
```

A Rust host names its choice by what it builds against. Every payload position
is bytes by default, and the `msgpack` feature adds the bundled codec's
spelling of each — `ValueReceiver` plus `ValueAdapter`, `RunPayload::values`,
`Yielder::call`, `Execution::value`. A host with its own schema turns the
feature off and implements the byte-level surface. A verb belongs to no
spelling: `run` takes whichever payload it is handed, so the feature governs
what a host is offered, never what it can reach.

**Building without one.** Replaceability is a property of the dependency
graph, not a flag. `kobako-transport` carries no payload codec at all and
`kobako-core` depends on nothing else, so a guest that only routes messages
reaches neither MessagePack nor `kobako-codec`. `kobako-mruby` is the same:
naming the harness pulls in no codec, and its `msgpack` feature is what adds
`MsgpackCodec` for a shell that wants one — the shipped `kobako-wasm` shell
asks for it, and a shell naming its own `MrbGuest::Codec` never does. The Rust
host SDK is the frontend an embedder names directly rather than a tier someone
else composes, so it defaults to a codec: `kobako`'s `msgpack` feature is on by
default and carries the whole `Value` surface, and with it off the crate
reaches no payload codec either.

kobako builds each of those tiers on every release — the guest tiers on the
build a third party gets, the SDK with its codec deselected — and checks that
no codec appears in the resulting graph, so this is a claim held to a build
rather than stated.

## Capability set

`MrbGuest::init_gems` installs the shell's gems onto the freshly booted VM.
Each gem is a `beni::Gem`; `kobako-io`, `kobako-regexp`, and `kobako-json` are
the worked examples, and each is free of any dependency on `kobako-mruby`.
Returning `Ok(())` yields a bridge-only guest — the wire-tied `KobakoBridge`
installs itself before the hook runs.

A gem that reaches the host rather than staying in-guest names one more tier:
`kobako-mruby`, whose `dispatch` rounds one Call through the host. A method
that takes a block hands it to the same call — the block parks for the call's
duration there, because the host's yield re-enters through a separate export
while the dispatch frame is still on the stack. Passing it is the whole
obligation; a guest that is not mruby reaches `kobako-core`'s
`transport::proxy::dispatch` directly and states the `block_given` bit itself.

That tier carries no payload codec, so a gem that reaches the wire still names
its own schema. It encodes the payload before calling and decodes the answer
after — which is also what keeps a guest-side raise clear of the parked block.

## Invocation flows

`MrbGuest` provides `eval` / `run` / `yield_to_block` over mruby. A shell that
implements one of them in its `Guest` impl rather than forwarding replaces that
flow. The obligation is the same either way: **write exactly one Outcome
envelope** per invocation entry.

## The whole guest

A non-mruby guest skips `kobako-mruby` entirely: implement
`kobako_core::Guest`, emit the exports with `export_guest!`, and the host
cannot tell the difference. This is the same path `wasm/kobako-wasm` takes —
the shipped shell is not privileged.

The obligations are the ABI's: six exports, one import, and the version the
host checks by equality (→ [`wire-codec.md`](wire-codec.md) § ABI Signatures).

## Wasm engine

`crates/kobako-runtime` is the engine-free contract: `Runtime`,
`DispatchHandler`, `Yielder`, the `Profile` a runtime declares, and the neutral
per-invocation types (`Snapshot`, `Completion`, `Capture`, `Usage`, `Trap`).
`crates/kobako-wasmtime` is one implementation of it. An engine that satisfies
the contract carries every frontend above it unchanged, because no frontend
names an engine type.

The Rust host SDK takes one at `Sandbox::with_runtime`, so a host that brings
its own engine keeps the whole tier above it — Catalog, Handles, snippet
replay, Extension composition. Only the isolation floor crosses that seam: the
engine's own caps are configured where the engine is built, and the SDK checks
the posture the engine declares against the floor the host asked for, refusing
construction below it rather than trusting the declaration. `Sandbox::new` is
the same path with the bundled wasmtime engine built for you.

The Ruby frontend takes no such seam. `Kobako::Runtime` is pinned to the
wasmtime driver, so engine choice there means choosing a different frontend.
