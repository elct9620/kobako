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
| Payload codec | `Receiver::call(.., payload: &[u8], ..)` | Rust host SDK |
| Capability set | `MrbGuest::init_gems` | guest |
| Invocation flow | a `Guest` method implemented rather than forwarded | guest |
| The whole guest | `impl kobako_core::Guest` + `export_guest!` | guest |
| Wasm engine | `impl Runtime` + `DispatchHandler` + `Yielder` | host |

Two things stay fixed. The **core envelope** and the **ABI surface** are the
same for every assembly — that is what makes the parts interchangeable at all
(→ [`wire-codec.md`](wire-codec.md)). And the **Ruby frontend is fixed to the
default codec**: MessagePack is Ruby's native choice and the gem speaks it
directly, so there is no seam to substitute at. A replacement codec is
installed on the guest and, if the host is Rust, at `Receiver`.

## Payload codec

A codec owns the payload positions and nothing else. The core envelope routes
and attributes without reading a payload byte, so replacing the codec leaves
the envelope, the ABI, and the version untouched.

**Obligations** (→ [`wire-codec.md`](wire-codec.md) § What a replacement codec
must provide):

| Position | What the codec must express |
|---|---|
| Call and Run payload | positional and keyword arguments, distinguishably |
| Reply fault body | the three reserved `type` values plus a message |
| Yield Reply ok / break body, Outcome result body | one value |

A Handle representation is optional. Without one, Handles ride only the
envelope's `target` field — a guest still reaches a stateful receiver and only
forgoes passing Handles as arguments.

**Where the choice is named.** A guest shell names it once, on the associated
type:

```rust
impl MrbGuest for MyGuest {
    type Codec = MyCodec;              // implements PayloadCodec
    fn init_gems(mrb: &Mrb) -> Result<(), beni::Error> { Ok(()) }
}
```

A Rust host reads the payload itself inside `Receiver::call`, which is handed
the bytes rather than decoded values. `ValueReceiver` plus `ValueAdapter` is
the shortcut for the default codec; a host with its own schema implements
`Receiver` directly.

**Building without one.** The routing-only tiers — `kobako-codec`,
`kobako-core`, `kobako-mruby` — build with `--no-default-features` and pull no
MessagePack dependency at all. `rake gate:payload:optional` holds them to it,
so "the codec is replaceable" stays a checked claim rather than a stated one.

## Capability set

`MrbGuest::init_gems` installs the shell's gems onto the freshly booted VM.
Each gem is a `beni::Gem`; `kobako-io`, `kobako-regexp`, and `kobako-json` are
the worked examples, and each is free of any dependency on `kobako-mruby`.
Returning `Ok(())` yields a bridge-only guest — the wire-tied `KobakoBridge`
installs itself before the hook runs.

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
