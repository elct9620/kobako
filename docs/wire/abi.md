# ABI

The ABI is the set of Wasm functions that carry both [wire](README.md) layers between host and guest, and the version that pins them.

| Section | Holds |
|---------|-------|
| ABI Signatures | the four symbols and the two layers they form |
| Channels | how a message and its answer cross |
| Invocation channels | what each invocation reads, and from where |
| ABI Version | what the version pins, and how it moves |

## ABI Signatures

The surface is four symbols. Signatures here use Wasm's own types; a Rust side spells the same values as `u32` and `u64`.

| Symbol | Linkage | Wasm signature | Layer |
|---|---|---|---|
| `__kobako_abi_version` | export | `() -> i32` | pre-message |
| `__kobako_alloc` | export | `(size: i32) -> i32` | pre-message |
| `__kobako_downcall` | export | `(ptr: i32, len: i32) -> i64` | message |
| `__kobako_upcall` | import | `(ptr: i32, len: i32) -> i64` | message |

The pre-message layer works before any message exists. The version is agreed before a tag can be read. `__kobako_alloc` gives the host guest memory to write a message into, answering `0` on failure. Adding, removing, or renaming a symbol is an ABI version increment (→ [`spec/behavior/runtime.md`](../spec/behavior/runtime.md)).

### Message Channels

Each direction has one channel, named for the way it crosses. An operation is a tag in the message rather than a symbol (→ [`envelope.md`](envelope.md) § Message Layer).

| Channel | Caller | Carries |
|---|---|---|
| `__kobako_downcall` | host | eval, run, yield |
| `__kobako_upcall` | guest | call |

1. The caller writes a message at `[ptr, ptr + len)` in guest memory.
2. The callee reads it, runs the operation its tag names, and encodes the answer.
3. The callee writes the answer into guest memory and returns its packed location.

The host writes into guest memory through `__kobako_alloc`. On a wire-layer fault in an upcall, such as a failed allocation, the host answers an empty packed value, which the guest refuses (→ [`spec/behavior/envelope.md`](../spec/behavior/envelope.md)).

### Call Nesting

Initiators alternate by depth, and every answer is the return value of the call at its depth. Correlation comes from the Wasm call stack, so no message carries an id.

```
downcall{run}                  <- host, depth 0
`- upcall{call}                <- guest, depth 1
   `- downcall{yield}          <- host, depth 2, inside the Service's yield
      `- upcall{call}          <- guest, depth 3
```

A yield nests inside the upcall the host is still answering. The Yield Call and its answer are in [`envelope.md`](envelope.md) § Yield Messages.

### Packed Return

Both channels return a location packed as two u32 values in one i64. Every pointer refers to guest linear memory.

```
 63        32 31         0
 ┌──────────┬────────────┐
 │   ptr    │    len     │
 └──────────┴────────────┘
 high 32 bits  low 32 bits
```

The reader copies the answer during the call frame and keeps no reference to it. Buffers are never freed one by one; the whole memory goes when the instance drops after the invocation.

---

## Invocation channels

An invocation reads its Frames from WASI stdin and its entry message from the downcall.

| Entry | WASI stdin frames | Downcall |
|---|---|---|
| eval | Frame 1 · Frame 3 | `eval` with the user source |
| run | Frame 1 · Frame 3 | `run` with the Run envelope |

A stdin frame is a `u32` big-endian length followed by that many bytes. Both frames are always sent, even empty, so no entry meets an end-of-input ambiguity. What each frame holds is in [`envelope.md`](envelope.md) § Invocation Frames.

---

## ABI Version

The ABI version is one u32 defined in `kobako-transport`, independent of every package version. The current version is `4`.

| Inside the version | Outside the version |
|--------------------|---------------------|
| the symbol set, names, signatures | a new request tag in the registry |
| the packed return convention | an additive contract change |
| the message framing, a redefined byte value, a new answer arm | a payload codec swap |

`__kobako_abi_version` is a constant function, callable before any entry runs, so one call can serve every Sandbox built from the artifact. The host accepts a Guest Binary only on equality, checked at Sandbox construction (→ [`spec/behavior/runtime.md`](../spec/behavior/runtime.md)).

### Lockstep Evolution

A host implements exactly one ABI version, so an increment is the cost of a change no reader survives.

| Rule | Consequence |
|------|-------------|
| No in-band version field | alignment checked once, not per message |
| No negotiation | each side implements one wire shape |
| Unsupported answer | a peer answers an unknown request, so a request tag joins without a bump |

A sandbox is short-lived, so no connection or stored payload outlasts a version. An independently built Guest Binary conforms by rebuilding against the new one. Release notes list wire-affecting changes under Breaking Changes.

### Version 4

Version `4` carries the operation in the message, so an added operation is a registry entry rather than a new symbol.

| Decision | Consequence |
|----------|-------------|
| one channel per direction | the symbol set stays at four |
| one tag space for every message | an answer's tag never collides across operations |
| every answer is a return value | no outcome buffer to drain after a call |
| field placement follows whose data it is | a Fault rides the envelope, readable without a codec |
| a fresh instance per invocation entry | the guest may exit with dirty interpreter state |
| boot state may live in data segments | the guest may arrive pre-initialized |
