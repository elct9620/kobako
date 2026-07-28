# Wire Codec

This document is the anchor for the binary encoding of the Wire Contract (→ `SPEC.md` § Wire Contract). It states how the two encoding layers relate, and holds the ABI surface that carries them. The byte-level references live in the two layer documents:

| Layer | Document | What it encodes | Who implements it |
|-------|----------|-----------------|-------------------|
| **Core envelope** | [`wire/envelope.md`](wire/envelope.md) | Fixed-layout frames — routing fields, ok-versus-fault, outcome attribution | `crates/kobako-transport`, shared by both sides |
| **Payload codec** | [`wire/payload-msgpack.md`](wire/payload-msgpack.md) | The opaque `payload` bytes each frame hands through — the type mapping and ext codes | `lib/kobako/` (host) ↔ `crates/kobako-codec` (guest) |

The governing summary of this codec lives in `SPEC.md` § Wire Codec; the abstract shape both layers encode is in [`wire-contract.md`](wire-contract.md).

ABI function names, packed return conventions, and the byte values stated in either layer document are fixed for the life of an ABI version and may only change together with an ABI version increment (→ § ABI Version).

---

## How the Two Layers Relate

The core envelope carries what routing and attribution need, and nothing else: a side resolves a Call's target, names its method, learns whether a Reply succeeded, and attributes a failed invocation — all without decoding a payload byte. Everything the resolved method actually consumes rides in an opaque `payload` field the codec owns.

Two properties follow, and they are the reason for the split:

- **The codec is replaceable.** A host and guest that agree on another schema swap [`wire/payload-msgpack.md`](wire/payload-msgpack.md) for their own and carry no MessagePack dependency. MessagePack is kobako's default payload codec, not the wire's only one. Each side names its choice at one seam — a Rust host at `Receiver`, a guest shell at `MrbGuest::Codec` — and the tiers beneath route messages without reading a payload byte, which `rake gate:payload:optional` holds them to.
- **The two decodes are separable.** Decoding an envelope requires nothing from the codec, and decoding a payload requires nothing from the envelope beyond its bytes and length. A frontend may split the two across its own internal boundaries, and an endpoint that only routes messages needs no codec at all.

A codec substitution changes neither the ABI surface below nor the envelope layout; a change to either of those is an ABI version increment.

### What a replacement codec must provide

The obligations are positions to fill, not an encoding to use. A codec that fills them interoperates with any kobako endpoint that speaks it.

| Obligation | Why the contract needs it |
|------------|---------------------------|
| Call and Run payloads express positional and keyword arguments distinguishably | The host dispatches through `public_send`, where the two are not interchangeable |
| A Yield Reply ok or break body and an Outcome result body each carry one value | These are single-value positions; a codec needs no framing beyond its own value encoding |

A Reply's fault body is not among them. Every byte of a Fault is kobako's — a closed category and a message — so it rides the envelope (→ [`wire/envelope.md`](wire/envelope.md) § Fault) and a replacement codec neither encodes nor reads one. A guest that speaks another schema still reads a refusal, and reads it without a codec at all.

A codec without a Handle representation is legal. Handles then ride only the envelope's `target` field, so a guest still reaches a stateful receiver and only forgoes passing Handles as arguments or receiving them as values.

---

## ABI Signatures

The following function names and byte-level signatures are fixed cross-implementer contracts. Implementers must not rename these functions or change their parameter or return types within an ABI version.

### Host-provided import

| Function name | Wasm signature | Return convention |
|---|---|---|
| `__kobako_dispatch` | `(req_ptr: i32, req_len: i32) -> i64` | Packed u64: high 32 bits = reply buffer ptr (zero-extended u32 wasm linear memory offset); low 32 bits = reply byte length (u32) |

The Guest Binary calls `__kobako_dispatch` after writing a Call envelope into linear memory at `[req_ptr, req_ptr + req_len)`. The Host Gem reads the envelope, dispatches it, serializes the Reply, allocates a buffer via `__kobako_alloc`, writes the Reply bytes into that buffer, and returns the packed i64. On any unrecoverable failure (allocation trap, serialization error, or an error outside the Reply fault arm), the import function returns an error to the Wasm engine, which surfaces as a Wasm trap and maps to `Kobako::TrapError`.

Single dispatch size limit: 16 MiB in either direction, applied to the whole envelope rather than the payload alone. Messages exceeding this limit are a wire violation; the Host Gem walks the trap path.

### Guest-provided exports

The ABI is a closed enumerated set: exactly six guest exports are permitted, listed below. No additional exports may be added without a new SPEC anchor that lifts the count.

| Export name | Wasm signature | Return convention |
|---|---|---|
| `__kobako_eval` | `() -> ()` | None — outcome is written to OUTCOME_BUFFER before return. Entry point for `Sandbox#eval`. |
| `__kobako_run` | `(env_ptr: i32, env_len: i32) -> ()` | None — outcome is written to OUTCOME_BUFFER before return. Entry point for `Sandbox#run`. `env_ptr` / `env_len` locate the Run envelope on the command buffer. |
| `__kobako_alloc` | `(size: i32) -> i32` | wasm linear memory offset (u32, unsigned); 0 indicates allocation failure (trap path) |
| `__kobako_take_outcome` | `() -> i64` | Packed u64: high 32 bits = OUTCOME_BUFFER ptr; low 32 bits = byte length. `len == 0` is a wire violation. |
| `__kobako_yield_to_block` | `(req_ptr: i32, req_len: i32) -> i64` | Packed u64: high 32 bits = Yield Reply buffer ptr; low 32 bits = Yield Reply byte length. `len == 0` is a wire violation. |
| `__kobako_abi_version` | `() -> i32` | u32 ABI version the Guest Binary was built against (→ § ABI Version) |

`__kobako_eval` and `__kobako_run` are the two invocation entry points. Both clear OUTCOME_BUFFER at entry, install the preamble (Frame 1), replay preloaded snippets (Frame 3), execute their verb-specific logic, and write a single Outcome envelope to OUTCOME_BUFFER before returning. The host then reads the envelope via `__kobako_take_outcome` and applies the two-step attribution decision (`SPEC.md` § Behavior; [`behavior/errors.md`](behavior/errors.md) § Error Scenarios).

The Host Gem calls `__kobako_yield_to_block` from inside a `__kobako_dispatch` callback when the Service method invokes its Yielder (B-24). The host writes the Yield Call — the yield arguments as a codec-encoded payload — into linear memory at `[req_ptr, req_ptr + req_len)`. The Guest Binary executes the block body within the active dispatch frame, allocates a buffer via `__kobako_alloc`, writes the Yield Reply bytes (→ [`wire/envelope.md`](wire/envelope.md) § Yield Call and Yield Reply), and returns the packed i64. The 16 MiB size limit applies in both directions.

### ABI Version

The ABI version is a single u32 owned by the SPEC corpus, independent of every package version (the kobako gem, any published crate). The current version is `4`.

`__kobako_abi_version` is a pure constant function: it takes no input, performs no I/O, touches no invocation state, and is callable before any invocation entry point runs. The Host Gem calls it once at Sandbox construction and compares the returned value against the version it implements by equality; an absent export or a non-equal value fails construction with `Kobako::SetupError` (B-40, E-42).

Any change to the Wire Contract, either layer document, or the ABI surface (function set, names, signatures) increments the version. There is no compatibility range and no negotiation: a host implements exactly one ABI version and loads only Guest Binaries reporting that version. Swapping the payload codec is not such a change — the codec is a choice the two endpoints share, outside the versioned surface.

Version `4` carries the two-layer wire: a fixed-layout core envelope with an opaque payload, and MessagePack as the default codec. It draws the line between the two by whose data a field is: a Reply's fault arm is kobako's own, so version `4` moved it out of the payload and into the envelope, where a guest reads a refusal with no codec at all. It also carries the per-invocation instance discipline ([`behavior/runtime.md`](behavior/runtime.md) B-49): the host drives every invocation entry on a fresh instance of the module and discards it after draining the outcome, so the Guest Binary may leave its interpreter state dirty at exit and may arrive with the canonical boot state pre-initialized in its data segments.

### Invocation channels

Each invocation entry point consumes a fixed sequence of inputs across two host→guest channels: WASI stdin (length-prefixed frames `[u32 be][bytes]`) and the command buffer (a Run envelope at `(ptr, len)` reachable via `__kobako_alloc` plus a linear-memory write, then surfaced as typed export arguments).

| Export | WASI stdin frames | Command buffer |
|---|---|---|
| `__kobako_eval` | Frame 1 preamble · Frame 2 user source · Frame 3 snippets | — |
| `__kobako_run` | Frame 1 preamble · Frame 3 snippets | Run envelope at `(env_ptr, env_len)` |

Frame 1 and Frame 3 are **mandatory-presence** even when empty: a Sandbox with no bindings sends an empty path list, and one with no preloads sends a zero-count snippet table, rather than an absent frame. That plus explicit empty payloads removes the `read_exact` EOF / partial-read ambiguity from each export's per-invocation contract. Frame 2 — the `#eval` user source — is raw UTF-8 bytes, read only by `__kobako_eval`, and loads with backtrace filename `(eval)`.

Layouts for Frame 1, Frame 3, and the Run envelope are in [`wire/envelope.md`](wire/envelope.md).

### Packed u64 return layout

`__kobako_dispatch`, `__kobako_take_outcome`, and `__kobako_yield_to_block` all return a packed i64 (Wasm type) carrying two u32 values:

```
 63        32 31         0
 ┌──────────┬────────────┐
 │   ptr    │    len     │
 └──────────┴────────────┘
 high 32 bits  low 32 bits
```

Extraction: `ptr = (result >> 32) & 0xffff_ffff`; `len = result & 0xffff_ffff`. The Wasm i64 is little-endian; the bit-shift extraction is portable across host environments.

Memory ownership: all buffer pointers refer to wasm linear memory owned by the Guest Binary Wasm instance. The Host Gem reads through a memory view provided by the Wasm engine during the call frame. After the call frame exits, the Host Gem holds no references to guest memory. Buffers are not individually freed; the entire wasm linear memory is released when the Wasm instance is dropped at the end of the invocation.

---

## Consistency Guarantee

Each layer is held to a second source that was not derived from its implementation. No layer's output is its own definition of correct.

| Layer | Second source | Mechanism |
|-------|---------------|-----------|
| Core envelope | [`wire/envelope.md`](wire/envelope.md) | Golden vectors, hand-derived from the layout document rather than from the code, pinning every frame it defines and every discriminant — each `kind` and `tag` byte — it fixes |
| Payload codec | A second implementation in another language | Bidirectional round-trip fuzz between `lib/kobako/` (Ruby) and `crates/kobako-codec` (Rust) |

The split follows where ambiguity lives. The type mapping — the 11 wire types, the two ext codes, the str/bin rules, the Symbol-keyed `kwargs` — is where two languages' conventions disagree, so that layer earns a second implementation. The envelope asks its implementers to agree on three routing fields and a byte string, and it is the fixed tier every assembly composes against, so the layout document is its second source and one definition is the guarantee.

A golden vector spells each discriminant as the literal byte [`wire/envelope.md`](wire/envelope.md) fixes, never as the constant the encoder reads it from: a vector written from that constant moves whenever the constant does, restating the implementation instead of holding it to anything.

The payload codec's fuzz contract is bidirectional and both directions are required:

- **Host → Guest → Host**: Host Gem encodes a payload → Guest Binary decodes and re-encodes → Host Gem decodes → deep equality with original.
- **Guest → Host → Guest**: Guest Binary encodes a payload → Host Gem decodes and re-encodes → Guest Binary decodes → deep equality with original.

Coverage must include all 11 wire types (→ [`wire/payload-msgpack.md`](wire/payload-msgpack.md) § Type Mapping), both ext types, and nested compositions (e.g., array of Handles, map with symbol keys, map containing bin values). Coverage must also pin the maximum nesting depth: a structure nested within the bound round-trips, and one beyond it — including a reference cycle — fails cleanly (E-06) rather than hard-trapping.

The core envelope's peers are both written in Rust, so this layer's cross-check is cross-implementation rather than cross-language. That is a deliberate trade: the type-mapping complexity that two languages disagree about lives entirely in the payload codec, where the Ruby↔Rust independence is retained in full, and the envelope is three routing fields plus a byte string.

Any failure at either layer is a wire regression that blocks release. The harness contract is specified in `SPEC.md` § Implementation Standards → Testing Style.
