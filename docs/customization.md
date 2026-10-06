# Customization

kobako is assembled from parts, and each assembly point is a named interface someone
outside this repository can implement. This document states what each one obliges
its implementer to do, and what kobako promises in return.

| Document | Covers |
|---|---|
| [`variants.md`](variants.md) | the prebuilt artifacts **we ship** |
| this document | the interfaces **a third party replaces** |
| [`architecture.md`](architecture.md) | which interfaces your starting point meets |

A variant is something to download; a customization point is something to
implement. Read the architecture map first, so you never implement a seam your
starting point already fixed.

## Commitment Grades

Public names declared here are graded commitments, not guidance. A grade says what
kobako owes you, and it is set by what you do with the name.

| Grade | What you do with it | What kobako promises |
|---|---|---|
| **stable** | write it in your own signatures | no change without the owning crate's version increment |
| **append-only** | match, implement, or read a set | the set only gains members |
| **exhaustive** | match a set completely | a new member breaks your match on purpose |
| **replaceable** | write your own in kobako's place | the obligations below are all you owe |

An append-only enum or read-only struct carries `#[non_exhaustive]`, and a new trait
method carries a default. An exhaustive set has neither, so there is no wildcard to
fall into. kobako swapping its own replaceable implementation is not a break.

Source compatibility is the owning crate's semantic version; wire compatibility is
the ABI version. A rename moves no byte, and a wire change need not touch a name.
Both govern only the fixed tier, where the shape of the name *is* the wire.

## Replaceable Points

Each row is one seam a third party can fill, with the endpoint it sits on.

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
| **stable** | `export_guest!` · `kobako_core::proxy::dispatch` · `kobako_core::abi::*` · `kobako_mruby::{Kobako, Arguments, dispatch}` · `kobako_runtime::{Snapshot, Capture, Usage, Frames}` · `kobako_codec::msgpack::{Encode, Decode}` · `kobako::{Sandbox, Options, Execution, Context, Handles, RunPayload, Backend}` · `kobako::handles::Detached` · `kobako_wasmtime::Config` |
| **stable · exhaustive** | `kobako_runtime::{Profile, Entry, Completion}` · `kobako::Provider` |
| **append-only** | `kobako_core::DispatchError` · `kobako_mruby::{CodecError, InstallError}` · `kobako_runtime::{Trap, SetupError, InvokeError}` · `kobako_codec::msgpack::Error` · `kobako::{Error, Failure, YieldError, Receiver, Extension}` · `kobako::msgpack::ValueReceiver` |
| **stable · exhaustive, and governed by the ABI version too** | `kobako_transport::abi::*` · `kobako_transport::envelope::*` · `kobako::FaultKind` |

Two things stay fixed. The **core envelope** and the **ABI surface** are the same for
every assembly, which makes the parts interchangeable at all
(→ [`wire-codec.md`](wire-codec.md)). Both live in `kobako-transport`, which depends
on nothing, so an implementer picks up the fixed tier without anyone else's choices.

The **Ruby frontend is fixed to the default codec**. MessagePack is Ruby's native
choice and the gem speaks it directly, so it has no seam to substitute at.

## Payload Codec

A codec owns the payload positions and nothing else. The core envelope routes and
attributes without reading a payload byte, so a new codec leaves the envelope, the
ABI, and the version untouched.

A guest fills the positions its codec serves. A Rust host reaches every position,
because each has a byte-level entry, and the `msgpack` feature adds the bundled
codec's spelling beside it.

| Position | Byte-level entry | `msgpack` spelling |
|---|---|---|
| dispatch arguments and answer | `Receiver` | `ValueReceiver`, `into_receiver` |
| `run` payload | `RunPayload::bytes`, `RunPayload::build` | `RunPayload::values` |
| yield | `Yielder::call_payload` | `Yielder::call_values` |
| invocation result | `Execution::payload` | `Execution::value` |
| object behind a Handle | `Handles::resolve`, `Execution::resolve` | `resolve_as` |

A Handle is resolved by id, since spelling a Handle is the schema's job and reaching
the object is not. Each spelling is a thin wrapper over its entry, so another
schema's overlay is written outside the SDK as extension traits over the same entries.

### Codec Obligations

A codec may serve only some positions; writing a value is the floor every codec owes
(→ [`wire-codec.md`](wire-codec.md#what-a-replacement-codec-must-provide)).

| Position | What serving it obliges |
|---|---|
| Yield Reply ok / break body, Outcome ok body | one value — **the floor** |
| Call payload | positional and keyword arguments, distinguishably |
| Run payload, Yield Call | the arguments alone; neither carries keywords |

How an unserved position refuses, and why a Call owes its Reply, is specified in
[codec](spec/behavior/codec.md). Keep the Call payload's keywords apart from its rest
arguments: a codec folding them together makes `KV.get(key, limit: 9)` lose `limit:`
with nothing raising.

A Handle representation is optional. Without one, Handles ride only the envelope's
`target` field: a guest still reaches a stateful receiver and only forgoes passing
Handles as arguments. A Reply's fault body is no codec position either. A Fault is
kobako's own data and rides the envelope, so a schema neither encodes nor reads one.

### Naming the Codec

A guest shell names its codec once, on the associated type.

```rust
impl MrbGuest for MyGuest {
    type Codec = MyCodec;              // implements PayloadCodec
    fn init_gems(mrb: &Mrb) -> Result<(), beni::Error> { Ok(()) }
}
```

A Rust host names its choice by what it builds against. Every `msgpack` spelling,
including the `Value` they speak, lives in the `msgpack` module rather than the crate
root. Turning the feature off removes a spelling rather than leaving a hole, and a
host with its own schema then implements the byte-level surface. `run` takes
whichever payload it is handed, so the feature governs what a host is offered, never
what it can reach.

A type implementing two schemas' receiver traits has two `into_receiver` in scope.
Name one (`ValueReceiver::into_receiver(kv)`): binding an object is choosing the
schema the guest reaches it through.

### Codec-free Builds

Replaceability is a property of the dependency graph, not a flag. Each tier reaches
a codec only where it asks for one.

| Crate | Codec it reaches |
|---|---|
| `kobako-transport`, `kobako-core` | none |
| `kobako-mruby` | `MsgpackCodec` only with its `msgpack` feature |
| `kobako-wasm` (shipped shell) | MessagePack, by asking for it |
| `kobako` (Rust host SDK) | the `Value` surface, `msgpack` on by default |

A shell naming its own `MrbGuest::Codec` never asks for `msgpack`. The SDK defaults
to a codec because an embedder names it directly; with the feature off it reaches no
payload codec.

`gate:payload:optional` builds each tier on every release and checks that no codec
appears in the graph. Each build includes the tier's tests, since a library whose
tests still need a codec has not moved it out of the code. A `[dev-dependencies]`
codec is allowed, because no consumer installs it.

## Capability Set

`MrbGuest::init_gems` installs the shell's gems onto the freshly booted VM. Each gem
is a `beni::Gem`; `kobako-io`, `kobako-regexp`, and `kobako-json` are the worked
examples, and none depends on `kobako-mruby`. A shell installing nothing still boots
(→ [mruby](spec/behavior/mruby.md)).

```rust
struct Greeter;

impl beni::Gem for Greeter {
    fn init(mrb: &Mrb) -> Result<(), beni::Error> {
        let class = mrb.define_class(c"Greeter", mrb.object_class())?;
        class.define_method(mrb, c"hello", beni::method!(hello, 1))?;
        Ok(())
    }
}

// A fixed argument list arrives as typed parameters; a failure goes back
// as `Err`, which beni raises at the guest call site.
fn hello(mrb: &Mrb, _self: Value, name: Value) -> Result<Value, beni::Error> {
    let who = String::try_convert(name, mrb)?;
    Ok(mrb.str_new(format!("hello {who}").as_bytes()).as_value())
}
```

That is the whole of a gem. It ships no Ruby source and needs no `mrbc` pipeline, so
the surface a guest sees is the one the Rust file registers.

### Carrying Rust Data

A class that carries Rust data rather than Ruby state is declared with
`#[beni::wrap]`, and the gem marks its carriers as it installs. The carrier-class
rules themselves are beni's.

```rust
#[beni::wrap(class = "Counter", name = "Greeter::Counter")]
struct Counter {
    hits: u32,
}

// inside the gem's init
mrb.define_class(c"Counter", mrb.object_class())?;
Counter::mark_carriers(mrb)?;
```

### Reaching the Host

A gem that reaches the host names one more tier: `kobako-mruby`, whose `dispatch`
rounds one Call through the host. A method taking a block passes it along, as the
block read off its own frame or `None`; that is the whole obligation. A guest that
is not mruby calls `kobako_core::proxy::dispatch` directly and states the
`block_given` bit itself.

```text
  gem ──► dispatch(target, method, Option<Proc>, payload) ──► Service
           │                                                    │
           └─ block parks for the call ◄── yield ───────────────┘
              (a separate export re-enters while the dispatch
               frame is still on the stack)
```

That tier carries no payload codec, so the gem still names its own schema. It
encodes the payload before calling and decodes the answer after, which keeps a
guest-side raise clear of the parked block.

## Guest Replacement

A shell replaces one invocation flow, or the whole guest, and the host cannot tell
the difference.

| Replace | Implement | Obligation |
|---|---|---|
| one invocation flow | that `Guest` method, not forwarded | exactly one Outcome envelope per entry |
| the whole guest | `kobako_core::Guest` + `export_guest!` | the ABI's exports, import, and version |

`MrbGuest` provides `eval`, `run`, and `yield_to_block` over mruby. A non-mruby guest
skips `kobako-mruby` entirely. `wasm/kobako-wasm` takes this same path, so the
shipped shell is not privileged. The ABI is in
[`wire-codec.md`](wire-codec.md#abi-signatures).

## Wasm Engine

`crates/kobako-runtime` is the engine-free contract, and `crates/kobako-wasmtime` is
one implementation of it. No frontend names an engine type, so an engine satisfying
the contract carries every frontend above it unchanged.

| Contract part | Names |
|---|---|
| engine seams | `Runtime`, `DispatchHandler`, `Yielder` |
| declared posture | `Profile` |
| per-invocation types | `Snapshot`, `Completion`, `Capture`, `Usage`, `Trap` |

The Rust host SDK takes an engine at `Sandbox::with_runtime` and keeps the whole tier
above it: Catalog, Handles, snippet replay, Extension composition. `Sandbox::new` is
the same path with the bundled wasmtime engine.

Only the isolation floor crosses that seam. The engine's own caps are configured
where the engine is built, while the SDK checks the posture it declares
(→ [runtime](spec/behavior/runtime.md)).

### Engine-free Builds

As with the codec, the engine's replaceability is a property of the dependency graph.

| `kobako` build | Way in |
|---|---|
| `wasmtime` feature on (default) | `Sandbox::new` or `Sandbox::with_runtime` |
| `wasmtime` feature off | `Sandbox::with_runtime` only; no engine reached |

`gate:engine:optional` builds the SDK that way on every release and checks that no
engine appears in the graph.

The Ruby frontend takes no such seam. `Kobako::Runtime` is pinned to the wasmtime
driver, so choosing an engine there means choosing a different frontend.
