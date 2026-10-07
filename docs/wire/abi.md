# ABI

The ABI is the set of Wasm functions that carry both [wire](README.md) layers between host and guest, and the version that pins them.

| Section | Holds |
|---------|-------|
| ABI Signatures | the import, the exports, and what each entry point does |
| Invocation channels | what each entry point reads, and from where |
| ABI Version | what the version pins, and how it moves |

## ABI Signatures

### Host-provided import

The host provides one import, fixed within an ABI version. Signatures here use Wasm's own types; a Rust side spells the same values as `u32` and `u64`.

| Function | Wasm signature | Returns |
|---|---|---|
| `__kobako_dispatch` | `(req_ptr: i32, req_len: i32) -> i64` | packed Reply location |

1. The Guest Binary writes a Call envelope at `[req_ptr, req_ptr + req_len)` and calls the import.
2. The host decodes and dispatches the Call, then encodes the Reply.
3. The host allocates a guest buffer through `__kobako_alloc` and writes the Reply into it.
4. The host returns the packed location.

On a wire-layer fault, such as a failed allocation, the host answers an empty packed value. The guest refuses it as an envelope failure (→ [`spec/behavior/envelope.md`](../spec/behavior/envelope.md)).

### Guest-provided exports

The guest exports exactly six functions; adding, removing, or renaming one is an ABI version increment (→ [`spec/behavior/runtime.md`](../spec/behavior/runtime.md)).

| Export | Wasm signature | Returns |
|---|---|---|
| `__kobako_eval` | `() -> ()` | nothing; `Sandbox#eval` entry |
| `__kobako_run` | `(env_ptr: i32, env_len: i32) -> ()` | nothing; `Sandbox#run` entry |
| `__kobako_alloc` | `(size: i32) -> i32` | a linear-memory offset, `0` on failure |
| `__kobako_take_outcome` | `() -> i64` | packed Outcome location |
| `__kobako_yield_to_block` | `(req_ptr: i32, req_len: i32) -> i64` | packed Yield Reply location |
| `__kobako_abi_version` | `() -> i32` | the ABI version |

An empty answer from either packed export is refused (→ [`spec/behavior/outcome.md`](../spec/behavior/outcome.md), [`spec/behavior/transport-yield.md`](../spec/behavior/transport-yield.md)).

### Invocation entry points

`__kobako_eval` and `__kobako_run` each write exactly one Outcome envelope before returning. Each runs these steps:

1. Install the preamble (Frame 1).
2. Replay preloaded snippets (Frame 3).
3. Run the verb-specific logic.
4. Write the Outcome to the outcome buffer.

The host drains the Outcome through `__kobako_take_outcome`, and a trap outranks any Outcome written (→ [`spec/behavior/outcome.md`](../spec/behavior/outcome.md)).

### Block yields

The host calls `__kobako_yield_to_block` when a Service invokes its Yielder. The call nests inside the dispatch frame the host is still answering.

```
guest ──__kobako_dispatch──▶ host Service
guest ◀─__kobako_yield_to_block── Yielder   (Yield Call at req_ptr)
guest runs block, writes Yield Reply via __kobako_alloc
guest ──packed i64──▶ host
```

The Yield Call and Yield Reply layouts are in [`envelope.md`](envelope.md).

### Packed Return

The three location-returning functions pack two u32 values into one i64. Every pointer refers to guest linear memory.

```
 63        32 31         0
 ┌──────────┬────────────┐
 │   ptr    │    len     │
 └──────────┴────────────┘
 high 32 bits  low 32 bits
```

The host reads a location only during the call frame and keeps no reference to it. Buffers are never freed one by one; the whole memory goes when the instance drops after the invocation.

---

## Invocation channels

Each entry point reads a fixed sequence of inputs across two host→guest channels.

| Export | WASI stdin frames | Command buffer |
|---|---|---|
| `__kobako_eval` | Frame 1 · Frame 2 · Frame 3 | — |
| `__kobako_run` | Frame 1 · Frame 3 | the Run envelope |

A stdin frame is a `u32` big-endian length followed by that many bytes. Frame 1 and Frame 3 are always sent, even empty, so no export meets an end-of-input ambiguity. The Run envelope reaches the command buffer through `__kobako_alloc` and a linear-memory write. What each frame holds is in [`envelope.md`](envelope.md) § Invocation Frames.

---

## ABI Version

The ABI version is one u32 defined in `kobako-transport`, independent of every package version. The current version is `3`.

| Inside the version | Outside the version |
|--------------------|---------------------|
| the export and import set, names, signatures | an additive contract change |
| the packed return convention | a payload codec swap |
| any redefined field or byte value | |

`__kobako_abi_version` is a constant function, callable before any entry point runs, so one call can serve every Sandbox built from the artifact. The host accepts a Guest Binary only on equality, checked at Sandbox construction (→ [`spec/behavior/runtime.md`](../spec/behavior/runtime.md)).

### Lockstep Evolution

A host implements exactly one ABI version, so an increment is the cost of a change no reader survives.

| Rule | Consequence |
|------|-------------|
| No in-band version field | alignment checked once, not per message |
| No negotiation | each side implements one wire shape |
| Lockstep evolution | a field change bumps the version on both sides |

A sandbox is short-lived, so no connection or stored payload outlasts a version. An independently built Guest Binary conforms by rebuilding against the new one. Release notes list wire-affecting changes under Breaking Changes.

### Version 3

Version `3` is the two-layer wire with MessagePack as the default codec, plus the per-invocation instance discipline.

| Decision | Consequence |
|----------|-------------|
| field placement follows whose data it is | a Fault rides the envelope, readable without a codec |
| a fresh instance per invocation entry | the guest may exit with dirty interpreter state |
| boot state may live in data segments | the guest may arrive pre-initialized |
