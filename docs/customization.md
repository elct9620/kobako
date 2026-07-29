# Customization — the interfaces a third party replaces

kobako is assembled from parts, and each assembly point is a named interface
someone outside this repository can implement. This document states what each
one obliges its implementer to do, and what kobako promises in return.

The boundary with [`variants.md`](variants.md): variants are the prebuilt
artifacts **we ship** — a matrix of capability gems composed onto one base.
Customization is the interfaces **a third party replaces**. A variant is
something to download; a customization point is something to implement.

Which of these interfaces you will meet at all depends on where you build
from; [`architecture.md`](architecture.md) is that map, and reading it first
saves implementing a seam your starting point already fixed.

Public names declared here are graded commitments, not guidance (N-9). A grade
says what kobako owes you, and it is set by what you do with the name:

| Grade | What you do with it | What kobako promises |
|---|---|---|
| **stable** | write the name in your own signatures | it does not change name or shape without a version increment of the crate that owns it |
| **append-only** | match it or implement it, and it is a set — an enum's variants, a trait's methods | the set only gains members; existing ones do not move. Enums carry `#[non_exhaustive]`, new trait methods carry a default |
| **replaceable** | write your own in place of kobako's | the obligations below are the whole of what your implementation owes; kobako swapping its own implementation is not a break |

Source compatibility is the owning crate's semantic version; wire
compatibility is the ABI version. They are separate — a rename moves no byte,
and a wire change need not touch a name. The one group both govern is the
fixed tier, where the shape of the name *is* the wire.

## What is replaceable

| Point | Interface | Endpoint | Grade |
|---|---|---|---|
| Payload codec | `MrbGuest::Codec: PayloadCodec` | guest | replaceable · methods append-only |
| Payload codec | the byte-level payload surface, `msgpack` feature off | Rust host SDK | stable |
| Capability set | `MrbGuest::init_gems` | guest | replaceable |
| Invocation flow | a `Guest` method implemented rather than forwarded | guest | replaceable · methods append-only |
| The whole guest | `impl kobako_core::Guest` + `export_guest!` | guest | replaceable · methods append-only |
| Wasm engine | `impl Runtime`, handed to `Sandbox::with_runtime` | Rust host SDK | replaceable |
| Wasm engine | `impl Runtime` + `DispatchHandler` + `Yielder` | a host frontend of your own | replaceable |

The types those seams carry, by grade:

| Grade | Names |
|---|---|
| **stable** | `export_guest!` · `kobako_core::proxy::dispatch` · `kobako_core::abi::*` · `kobako_mruby::{Kobako, Arguments, dispatch}` · `kobako_runtime::{Profile, Snapshot, Capture, Completion, Usage, Entry, Frames}` · `kobako::{Sandbox, Options, Execution, Context, Handles, RunPayload}` |
| **append-only** | `kobako_core::DispatchError` · `kobako_mruby::CodecError` · `kobako_runtime::{Trap, SetupError, InvokeError}` · `kobako::{Error, Failure, YieldError}` |
| **stable, and governed by the ABI version too** | `kobako_transport::abi::*` · `kobako_transport::envelope::*` · `kobako::FaultKind` |

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

The two endpoints reach the same set of positions. A guest fills the ones its
codec serves; a Rust host built on the SDK reaches every one of them, because
each has a byte-level entry there — a dispatch's arguments and answer at
`Receiver`, a `run` payload at `RunPayload::bytes` or `RunPayload::build`, a yield at
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

| Position | What serving it obliges |
|---|---|
| Yield Reply ok / break body, Outcome ok body | one value — **the floor**, and the only position every codec owes |
| Call payload | positional and keyword arguments, distinguishably |
| Run payload, Yield Call | the arguments alone; neither carries keywords |

A codec may serve only some of these. Every invocation ends by writing an
Outcome, so writing a value is the one thing none of them can leave out;
the rest are capabilities a codec chooses. A position it does not serve
refuses there, and the refusal says the position is unserved rather than
that a message was unreadable — a script sees `NotImplementedError` where
it reached for a capability this guest does not have. A Call payload and
its Reply value are two halves of one exchange, so serving either owes the
other; nothing enforces that, and a codec that breaks the pairing leaves
the exchange half-served at the Reply.

Only the Call payload owes the distinction, and it owes it because that is
where keywords exist: the guest hands the codec a separated rest slice and
keyword Hash, so a codec that folds them together makes `KV.get(key, limit: 9)`
lose `limit:` with nothing raising. A `#run` payload's keywords ride as a
trailing Hash the entrypoint reads positionally, and a Yield Call's arguments
are a plain list — neither position can lose a keyword, because neither carries
one.

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
spelling of each in a `msgpack` module — `ValueReceiver` and its
`into_receiver`, `RunPayload::values`, `Yielder::call_values`,
`Execution::value`, `resolve_as` for reaching a bound object back through the
seam `into_receiver` puts in front of it, and the `Value` they all speak. Every
one of them lives in that module rather than at the crate root, which is what
makes turning the feature off remove a spelling rather than leave a hole. A
host with its own schema turns it off and implements the byte-level surface. A
verb belongs to no spelling: `run` takes whichever payload it is handed, so the
feature governs what a host is offered, never what it can reach.

A type implementing two schemas' receiver traits has two `into_receiver` in
scope, and a call is ambiguous until one is named
(`ValueReceiver::into_receiver(kv)`). Binding an object is choosing the schema
the guest reaches it through, so that is the choice being asked for rather than
a collision to work around.

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
rather than stated. Each build includes the tier's tests: a library that
compiles codec-free while its own tests do not has moved the codec out of the
shipped graph without moving it out of the code. A test may still reach a codec
through a `[dev-dependencies]` entry, which no consumer installs.

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

**Building without one.** As with the codec, the engine's replaceability is a
property of the dependency graph. `kobako`'s `wasmtime` feature is on by
default and carries `Sandbox::new`; with it off the crate reaches no engine at
all, and `with_runtime` is the only way in. kobako builds it that way on every
release and checks that no engine appears in the resulting graph.

The Ruby frontend takes no such seam. `Kobako::Runtime` is pinned to the
wasmtime driver, so engine choice there means choosing a different frontend.
