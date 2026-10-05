# Wire Codec

This document anchors the binary encoding of the [Wire Contract](wire-contract.md). It relates the two encoding layers and holds the ABI surface that carries them.

| Layer | Byte reference | Encodes | Implemented in |
|-------|----------------|---------|----------------|
| Core envelope | [`wire/envelope.md`](wire/envelope.md) | routing, ok-versus-fault, attribution | `crates/kobako-transport`, both sides |
| Payload codec | [`wire/payload-msgpack.md`](wire/payload-msgpack.md) | the opaque `payload` bytes | `lib/kobako/` ↔ `crates/kobako-codec` |

ABI function names, packed return conventions, and the byte values in either layer document change only with an ABI version increment (→ § ABI Version). A field or closed-set value the contract adds is not such a change: a reader degrades what it predates (→ [`wire-contract.md`](wire-contract.md) § Fault).

---

## How the Two Layers Relate

The core envelope carries only what routing and attribution need, so a side reaches a decision without decoding a payload byte. Everything the resolved method consumes rides in the opaque `payload` the codec owns.

```
┌───────────── core envelope ─────────────┐
│ routing fields │ tag │ payload (opaque) │
└──────────────────────────────┬──────────┘
                               └── payload codec
```

| Property | Consequence |
|----------|-------------|
| Replaceable codec | a shared schema swaps out MessagePack entirely |
| Separable decodes | a routing-only endpoint needs no codec |

MessagePack is the default codec, not the only one. A guest shell names its codec at `MrbGuest::Codec`; a Rust host builds the SDK without its `msgpack` feature. `rake gate:payload:optional` checks that no codec appears in either side's codec-free graph. A codec substitution changes neither the ABI surface nor the envelope layout.

### What a replacement codec must provide

A codec fills positions, not an encoding. The floor is unconditional. Each other position is a capability the codec serves or refuses as unserved, so a missing feature reads apart from a broken guest (→ [`spec/behavior/codec.md`](spec/behavior/codec.md)).

| Position | Serving it obliges | Why |
|----------|--------------------|-----|
| Outcome ok; Yield Reply ok or break | one value | the floor: every invocation writes an Outcome |
| Call payload | positional and keyword arguments, apart | `public_send` does not interchange them |
| Run payload; Yield Call | the arguments alone | neither position has keywords |

A Call payload and its Reply value are two halves of one exchange, so a codec serving either owes the other. A Fault is kobako's own, so it rides the envelope and no codec encodes one (→ [`wire/envelope.md`](wire/envelope.md) § Fault).

A codec without a Handle representation is legal. Handles then ride only the envelope's `target` field, so a guest still reaches a stateful receiver and only forgoes Handles as arguments or values.

---

## ABI Signatures

### Host-provided import

The host provides one import; its name and signature are fixed within an ABI version.

| Function name | Wasm signature | Return convention |
|---|---|---|
| `__kobako_dispatch` | `(req_ptr: i32, req_len: i32) -> i64` | packed Reply ptr and length (§ Packed u64 return layout) |

1. The Guest Binary writes a Call envelope at `[req_ptr, req_ptr + req_len)` and calls the import.
2. The host decodes and dispatches the Call, then serializes the Reply.
3. The host allocates a guest buffer via `__kobako_alloc` and writes the Reply into it.
4. The host returns the packed i64.

On a wire-layer fault, such as a failed allocation, the host answers an empty packed answer. The guest refuses it as an envelope failure (→ [`spec/behavior/envelope.md`](spec/behavior/envelope.md)). Every message obeys the 16 MiB bound (→ [`wire/envelope.md`](wire/envelope.md) § Size and Depth Bounds).

### Guest-provided exports

The guest exports a closed set of six functions; adding, removing, or renaming one is an ABI version increment (→ [`spec/behavior/runtime.md`](spec/behavior/runtime.md)).

| Export name | Wasm signature | Return convention |
|---|---|---|
| `__kobako_eval` | `() -> ()` | none; `Sandbox#eval` entry |
| `__kobako_run` | `(env_ptr: i32, env_len: i32) -> ()` | none; `Sandbox#run` entry, Run envelope location |
| `__kobako_alloc` | `(size: i32) -> i32` | linear-memory offset (u32); `0` on failure |
| `__kobako_take_outcome` | `() -> i64` | packed OUTCOME_BUFFER ptr and length |
| `__kobako_yield_to_block` | `(req_ptr: i32, req_len: i32) -> i64` | packed Yield Reply ptr and length |
| `__kobako_abi_version` | `() -> i32` | u32 ABI version (§ ABI Version) |

An empty answer from either packed export is refused (→ [`spec/behavior/outcome.md`](spec/behavior/outcome.md), [`spec/behavior/transport-yield.md`](spec/behavior/transport-yield.md)).

### Invocation entry points

`__kobako_eval` and `__kobako_run` each write exactly one Outcome envelope before returning. Each export runs these steps:

1. Clear OUTCOME_BUFFER.
2. Install the preamble (Frame 1).
3. Replay preloaded snippets (Frame 3).
4. Run the verb-specific logic.
5. Write the Outcome to OUTCOME_BUFFER.

The host drains the Outcome through `__kobako_take_outcome`, and a trap outranks any Outcome written (→ [`spec/behavior/outcome.md`](spec/behavior/outcome.md)).

### Block yields

The host calls `__kobako_yield_to_block` from inside a `__kobako_dispatch` callback when a Service invokes its Yielder. The call nests inside the dispatch frame the host is still answering:

```
guest ──__kobako_dispatch──▶ host Service
guest ◀─__kobako_yield_to_block── Yielder   (Yield Call at req_ptr)
guest runs block, writes Yield Reply via __kobako_alloc
guest ──packed i64──▶ host
```

The Yield Reply layout is in [`wire/envelope.md`](wire/envelope.md) § Yield Call and Yield Reply.

### ABI Version

The ABI version is a single u32 defined once in `kobako-transport`, independent of every package version. The current version is `3`.

`__kobako_abi_version` is a pure constant function, callable before any invocation entry point runs. The host compares it by equality at Sandbox construction (→ [`spec/behavior/runtime.md`](spec/behavior/runtime.md)). The answer is a property of the artifact, so one call may serve every Sandbox built from it.

| Inside the version | Outside the version |
|--------------------|---------------------|
| the export and import set, names, signatures | an additive contract change |
| the packed return conventions | a payload codec swap |
| any redefined field or byte value | |

A host implements exactly one ABI version, so an increment is the cost of a change no reader survives.

### Version 3

Version `3` is the two-layer wire with MessagePack as the default codec. It also carries the per-invocation instance discipline.

| Decision | Consequence |
|----------|-------------|
| field placement follows whose data it is | a Fault rides the envelope, readable without a codec |
| a fresh instance per invocation entry | the guest may exit with dirty interpreter state |
| boot state may live in data segments | the guest may arrive pre-initialized |

### Invocation channels

Each invocation entry point reads a fixed sequence of inputs across two host→guest channels.

| Export | WASI stdin frames | Command buffer |
|---|---|---|
| `__kobako_eval` | Frame 1 preamble · Frame 2 user source · Frame 3 snippets | — |
| `__kobako_run` | Frame 1 preamble · Frame 3 snippets | Run envelope at `(env_ptr, env_len)` |

A stdin frame is `[u32 be][bytes]`. The Run envelope reaches the command buffer through `__kobako_alloc` and a linear-memory write. Frame 1 and Frame 3 are always sent, even empty, so no export meets an EOF or partial-read ambiguity. Frame 2 is raw UTF-8 source read only by `__kobako_eval`. Frame layouts are in [`wire/envelope.md`](wire/envelope.md).

### Packed u64 return layout

`__kobako_dispatch`, `__kobako_take_outcome`, and `__kobako_yield_to_block` each return an i64 packing two u32 values:

```
 63        32 31         0
 ┌──────────┬────────────┐
 │   ptr    │    len     │
 └──────────┴────────────┘
 high 32 bits  low 32 bits
```

Extract with `ptr = (result >> 32) & 0xffff_ffff` and `len = result & 0xffff_ffff`; the shift is portable across hosts.

Every pointer refers to guest linear memory. The host reads it only during the call frame and keeps no reference afterwards. Buffers are never freed one by one; the whole memory goes when the instance drops after the invocation.

---

## Consistency Guarantee

Each layer is held to a second source not derived from its implementation, so no layer's output defines its own correctness.

| Layer | Second source | Mechanism |
|-------|---------------|-----------|
| Core envelope | [`wire/envelope.md`](wire/envelope.md) | golden vectors per frame, `kind` and `tag` byte |
| Payload codec | an implementation in another language | round-trip fuzz, Ruby ↔ Rust |

The split follows where ambiguity lives. The type mapping is where two languages' conventions disagree, so that layer earns a second implementation. The envelope's peers are both Rust and share a few routing fields, so its layout document is its second source. A failure at either layer is a wire regression that blocks release.

### Golden Vectors

A golden vector spells each discriminant as the literal byte the layout document fixes. A vector written from the encoder's constant would only restate the implementation.

```
wire/envelope.md --hand-derived--> golden vector <--compared-- kobako-transport
```

### Round-Trip Fuzz

The payload codec's fuzz harness holds both peers to each other over bytes, however they are connected:

1. Run Host → Guest → Host and Guest → Host → Guest; each ends in deep equality with the original.
2. Cover all 11 wire types, both ext types, and nested compositions such as an array of Handles.
3. Round-trip a structure at the nesting bound, and fail cleanly — never trap — one past it, a reference cycle included.
4. Take the seed from an environment variable, and print it in every failure so the seed alone reproduces the run.
5. Fail the run when any wire type or ext type went unobserved, independent of byte equality.

Iteration count is the implementer's choice. The type mapping lives in [`wire/payload-msgpack.md`](wire/payload-msgpack.md) § Type Mapping.
