# Wire Codec

This document pins the binary encoding of the Wire Contract (→ `SPEC.md` § Wire Contract). Both the Host Gem and the Guest Binary implement this encoding independently; the codec form is a public cross-implementer contract. Byte values, ext type codes, ABI function names, and packed return conventions stated here are fixed for the life of a kobako gem release and may only change in a release that simultaneously updates both sides.

The governing summary of this codec lives in `SPEC.md` § Wire Codec; this document is its byte-level reference.

---

## Codec Choice

MessagePack is the wire codec. It is the only codec used on either side of the Wasm boundary; no fallback or alternative codec is permitted. All messages — Requests, Responses, and Outcome envelopes — are MessagePack-encoded byte sequences.

---

## Type Mapping

The following 12 entries constitute the complete set of MessagePack types recognized on the kobako wire. Any msgpack type or ext code not listed here is a wire violation; both sides reject it without attempting to decode the payload.

| # | msgpack family | Wire use | Host Gem Ruby type | Guest Binary mruby / Rust type |
|---|----------------|----------|--------------------|-------------------------------|
| 1 | nil | Absent optional fields; explicit `nil` values | `nil` | `nil` (mruby) / `Option::None` |
| 2 | bool | Boolean values | `true` / `false` | `TrueClass` / `FalseClass` (mruby) / `bool` |
| 3 | int (all widths: fixint, int 8/16/32/64, uint 8/16/32/64) | Integer values; `status` field (0 / 1) | `Integer` | `Integer` (mruby) / `i64` or `u64` |
| 4 | float (float 32 / float 64) | Floating-point values | `Float` | `Float` (mruby) / `f64` |
| 5 | str (fixstr / str 8 / str 16 / str 32) | UTF-8 text strings (see str/bin rules below) | `String` (UTF-8 encoding) | `String` (mruby) / `&str` / `String` |
| 6 | bin (bin 8 / bin 16 / bin 32) | Arbitrary byte sequences (see str/bin rules below) | `String` (binary / ASCII-8BIT encoding) | `String` (mruby, binary) / `&[u8]` / `Vec<u8>` |
| 7 | array (fixarray / array 16 / array 32) | Ordered sequences; Request / Response envelope framing | `Array` | `Array` (mruby) / `Vec<T>` |
| 8 | map (fixmap / map 16 / map 32) | Associative maps; `kwargs`; Panic envelope payload | `Hash` | `Hash` (mruby) / struct or `HashMap` |
| 9 | ext (general channel) | Dispatch point; kobako uses ext codes 0x00, 0x01, and 0x02; all other ext codes are wire violations | — (dispatch by code) | — (dispatch by code) |
| 10 | ext 0x00 | Symbol (see Ext Types below) | `Symbol` | `Symbol` (mruby `mrb_sym`) / `Sym(String)` |
| 11 | ext 0x01 | Capability Handle (see Ext Types below) | `Kobako::RPC::Handle` | `Kobako::RPC::Handle` (mruby) / `Handle(u32)` |
| 12 | ext 0x02 | Fault envelope (see Ext Types below) | `Kobako::RPC::Fault` (deserialized per error type, → `SPEC.md` § Error Classes) | `Errenv` struct |

---

## str / bin Encoding Rules

msgpack distinguishes `str` (UTF-8 text) from `bin` (raw bytes). The following rules govern which family is used at each wire position. A violation of a "str only" rule is a wire violation and the receiving side rejects the message.

| Wire position | Accepted family | Violation handling |
|---|---|---|
| Request `target` field (Member constant path form, e.g. `"Namespace::Member"`) | str only | bin → wire violation, reject |
| Request `method` field | str only | bin → wire violation, reject |
| Request `args` elements and `kwargs` values | str or bin (context-determined) | both are legal |
| Response Fault Envelope `type` field value | str only | bin → wire violation, reject |
| Response Fault Envelope `message` field value | str only | bin → wire violation, reject |
| Fault Envelope map keys (`type`, `message`, `details`) | str or bin (UTF-8 validated) | non-UTF-8 content → wire violation, reject |
| Panic Envelope `origin`, `class`, `message` field values | str only | bin → wire violation, reject |
| Panic Envelope map keys (`origin`, `class`, `message`, `backtrace`, `details`) | str or bin (UTF-8 validated) | non-UTF-8 content → wire violation, reject |

Symbols travel as ext 0x00 (→ Ext Types below). A Symbol encoded on one side and decoded on the other arrives as a Symbol with the same UTF-8 name; symbol identity across the wire is established by name equality, not by interned-id sharing. A `str` or `bin` value carrying the bytes of a symbol name is **not** wire-equivalent to that Symbol; the two are distinguishable on the wire and must remain distinguishable end-to-end.

---

## Ext Types

### ext 0x00 — Symbol

**Binary layout:** variable-length ext; framing is `ext 8` (format byte `0xc7`, 1-byte length, type byte `0x00`, payload) or `ext 16` (format byte `0xc8`, 2-byte big-endian length, type byte `0x00`, payload) depending on payload size. The payload is zero or more UTF-8 bytes — the symbol's name. An empty payload (`0xc7 0x00 0x00`) decodes as the empty Symbol (`:""`); this is wire-legal.

| Byte offset | Content |
|-------------|---------|
| 0 | `0xc7` or `0xc8` — msgpack `ext 8` / `ext 16` marker |
| 1 | length byte(s) — 1 byte for `ext 8`, 2 big-endian bytes for `ext 16` |
| n | `0x00` — kobako ext type code |
| n+1.. | UTF-8 bytes of the symbol name |

The payload bytes MUST decode as UTF-8. A non-UTF-8 payload is a wire violation: encoders SHOULD validate UTF-8 before emitting, and decoders MUST reject the message rather than fall back to a binary-encoded Symbol. The payload length is bounded only by msgpack's natural ext-family limits; kobako does not impose an additional cap.

Position rules for ext 0x00:

- **MUST be ext 0x00** at: Request `kwargs` map keys (no other wire type is accepted at this position; a `str`, `bin`, or other-type key is a wire violation).
- **MAY appear** at: Request `args` elements, Request `kwargs` values, Response `value` field (success variant), Result envelope `value` field, and as elements / keys / values of any nested array or map within those positions (other wire types are also permitted).
- **MUST NOT appear** at: Request `target` field, Request `method` field, Fault Envelope `type` / `message` fields, or Panic envelope `origin` / `class` / `message` fields.

### ext 0x01 — Capability Handle

**Binary layout:** fixed 4-byte payload, big-endian u32 Handle ID. The msgpack framing is `fixext 4`: format byte `0xd6`, type byte `0x01`, followed by 4 bytes of big-endian u32 data. Total wire size: 6 bytes.

| Byte offset | Content |
|-------------|---------|
| 0 | `0xd6` — msgpack `fixext 4` marker |
| 1 | `0x01` — kobako ext type code |
| 2–5 | Handle ID as big-endian u32 |

The Handle ID field carries the opaque identifier allocated by the HandleTable (→ `SPEC.md` § Wire Contract → Capability Handle). ID 0 is reserved as the invalid sentinel. The maximum valid ID is `0x7fff_ffff` (2³¹ − 1); any ID above this cap is a wire violation.

ext 0x01 may appear in: Request `target` field (Handle reference form), Request `args` elements, Response `value` field, Result envelope `value` field. It must not appear in any other position.

### ext 0x02 — Fault Envelope

**Binary layout:** variable-length ext; framing is `ext 8` (format byte `0xc7`, 1-byte length, type byte `0x02`, payload) or `ext 16` (format byte `0xc8`, 2-byte big-endian length, type byte `0x02`, payload) depending on payload size. The payload is an embedded msgpack **map** with exactly three keys:

| Map key | Value type | Meaning |
|---------|-----------|---------|
| `"type"` | str | One of the four reserved error type names: `"runtime"`, `"argument"`, `"disconnected"`, `"undefined"` (→ `SPEC.md` § Wire Contract → Fault Envelope) |
| `"message"` | str | Human-readable description |
| `"details"` | any wire-legal type, or nil | Structured supplementary information; nil or absent when not present |

ext 0x02 may appear only in the Response fault variant's envelope field. It must not appear in Request `args` or any other position.

---

## Envelope Encoding

Multi-field envelope frames — Request and Response — use msgpack **array** framing (not map). Fields are read and written by positional index; the wire carries no key strings. This means both sides must agree on field order; field order is fixed by this section and may not change within a release. The Panic envelope is encoded as a msgpack **map** keyed by name (see Panic Envelope below) because its fields (`origin`, `class`, `message`, `backtrace`, `details`) are forward-compatibility points where unknown keys must be silently ignored. The Result envelope carries a single value and is emitted as that value's msgpack encoding directly, without an enclosing array — the Outcome tag byte already discriminates the variant.

### Request

A 5-element msgpack array with fixed field positions:

| Index | Field | Type |
|-------|-------|------|
| 0 | `target` | str (Member constant path, e.g. `"Namespace::Member"`) or ext 0x01 (Capability Handle reference) |
| 1 | `method` | str |
| 2 | `args` | array (elements may include ext 0x01 Handles) |
| 3 | `kwargs` | map (str keys; empty kwargs is encoded as empty map `0x80`, never absent) |
| 4 | `has_block` | bool — `true` if the guest call site supplied a block (B-23); `false` otherwise |

The two forms of `target` are distinguishable at the first msgpack byte: a str family marker indicates a Member constant path; `0xd6` (fixext 4) indicates a Capability Handle reference. No additional union tag field is required.

### Response

A 2-element msgpack array with fixed field positions:

| Index | Field | Type |
|-------|-------|------|
| 0 | `status` | int — `0` (success) or `1` (error) |
| 1 | `value` (status=0) or fault envelope (status=1) | any wire-legal type including ext 0x01, or ext 0x02 |

### Result Envelope (Outcome payload — success)

The msgpack encoding of the user script's last mruby expression value, emitted directly without further framing. The Outcome tag byte (`0x01`) is the sole discriminator; no enclosing array is added.

| Field | Type |
|-------|------|
| `value` | any wire-legal type including ext 0x01 (if the script returned a stateful host object) |

### Panic Envelope (Outcome payload — failure)

A msgpack **map** (not array) with the following fields:

| Key | Value type | Meaning |
|-----|-----------|---------|
| `"origin"` | str | `"sandbox"` (mruby script error or boot fault) or `"service"` (unrescued Service failure) |
| `"class"` | str | Exception class name (e.g. `"RuntimeError"`, `"Kobako::ServiceError"`) |
| `"message"` | str | Exception message |
| `"backtrace"` | array of str | mruby backtrace; each element is one line |
| `"details"` | any wire-legal type, or nil | Optional structured data; nil or absent when not present |

Unknown map keys are silently ignored (forward-compatibility). Missing any of `"origin"`, `"class"`, or `"message"` is a wire violation; the Host Gem raises `Kobako::SandboxError` using a synthesized unknown-class fallback.

### Outcome Envelope

The Outcome envelope is the binary layout of OUTCOME_BUFFER — the shared memory region the Guest Binary writes at the end of an invocation export (`__kobako_eval` for `Sandbox#eval`, `__kobako_run` for `Sandbox#run`) and the Host Gem reads via `__kobako_take_outcome`. It wraps either a Result envelope or a Panic envelope under a one-byte tag:

| Byte offset | Content |
|-------------|---------|
| 0 | Tag byte: `0x01` = Result envelope follows; `0x02` = Panic envelope follows |
| 1 onwards | msgpack payload of the corresponding envelope |

Tag `0x01` example (script returns integer 42):

```
01 2a
│  └─ msgpack positive fixint 42 (the value, encoded directly)
└─ outcome tag 0x01 (result)
```

Tag `0x02` example (script raises `"boom"`):

```
02 84 a6 6f 72 69 67 69 6e ...
│  │
│  └─ msgpack fixmap len=4
└─ outcome tag 0x02 (panic)
```

Zero-length OUTCOME_BUFFER (`len == 0`) or any tag byte outside `{0x01, 0x02}` is a wire violation; the Host Gem raises `Kobako::TrapError` (wire-violation fallback, → `SPEC.md` § Error Scenarios → `Kobako::TrapError`).

### YieldResponse Envelope

The YieldResponse envelope is the byte layout returned from `__kobako_yield_to_block` (→ ABI Signatures). It is written into a Guest-Binary-allocated buffer the host reads after the yield re-entry returns. The envelope is a one-byte tag followed by an optional msgpack payload:

| Byte offset | Content |
|-------------|---------|
| 0 | Tag byte: `0x01` (ok), `0x02` (break), `0x03` (reserved — receivers reject as a wire violation), or `0x04` (error) |
| 1 onwards | msgpack payload (omitted entirely when the variant has no payload — currently no such variant) |

Tag `0x01` example (block returned `:done`):

```
01 a4 64 6f 6e 65
│  └─ msgpack fixstr len=4 "done"
└─ YieldResponse tag 0x01 (ok)
```

Tag `0x02` example (block executed `break :stop`):

```
02 a4 73 74 6f 70
│  └─ msgpack fixstr len=4 "stop"
└─ YieldResponse tag 0x02 (break)
```

Tag `0x04` example (block raised `RuntimeError`):

```
04 83 a5 63 6c 61 73 73 ...
│  │
│  └─ msgpack fixmap len=3 ({class, message, backtrace})
└─ YieldResponse tag 0x04 (error)
```

Zero-length YieldResponse, tag `0x03`, or any tag outside `{0x01, 0x02, 0x04}` is a wire violation; the Host Gem walks the trap path (→ `SPEC.md` § Wire Contract → YieldResponse Envelope).

---

## ABI Signatures

The following function names and byte-level signatures are fixed cross-implementer contracts. Implementers must not rename these functions or change their parameter or return types within a release.

### Host-provided import

| Function name | Wasm signature | Return convention |
|---|---|---|
| `__kobako_dispatch` | `(req_ptr: i32, req_len: i32) -> i64` | Packed u64: high 32 bits = response buffer ptr (zero-extended u32 wasm linear memory offset); low 32 bits = response byte length (u32) |

The Guest Binary calls `__kobako_dispatch` after writing a Request payload into linear memory at `[req_ptr, req_ptr + req_len)`. The Host Gem reads the Request, dispatches it, serializes the Response, allocates a response buffer via `__kobako_alloc`, writes the Response bytes into that buffer, and returns the packed i64. On any unrecoverable failure (allocation trap, serialization error, or an error outside the Response error-variant path), the import function returns an error to the Wasm engine, which surfaces as a Wasm trap and maps to `Kobako::TrapError`.

Single RPC payload size limit: 16 MiB in either direction. Payloads exceeding this limit are a wire violation; the Host Gem walks the trap path.

### Guest-provided exports

The ABI is a closed enumerated set: exactly five guest exports are permitted, listed below. No additional exports may be added without a new SPEC anchor that lifts the count.

| Export name | Wasm signature | Return convention |
|---|---|---|
| `__kobako_eval` | `() -> ()` | None — outcome is written to OUTCOME_BUFFER before return. Entry point for `Sandbox#eval`. |
| `__kobako_run` | `(env_ptr: i32, env_len: i32) -> ()` | None — outcome is written to OUTCOME_BUFFER before return. Entry point for `Sandbox#run`. `env_ptr` / `env_len` locate the invocation envelope on the command buffer. |
| `__kobako_alloc` | `(size: i32) -> i32` | wasm linear memory offset (u32, unsigned); 0 indicates allocation failure (trap path) |
| `__kobako_take_outcome` | `() -> i64` | Packed u64: high 32 bits = OUTCOME_BUFFER ptr; low 32 bits = byte length. `len == 0` is a wire violation. |
| `__kobako_yield_to_block` | `(req_ptr: i32, req_len: i32) -> i64` | Packed u64: high 32 bits = YieldResponse buffer ptr; low 32 bits = YieldResponse byte length. `len == 0` is a wire violation. |

`__kobako_eval` and `__kobako_run` are the two invocation entry points. Both clear OUTCOME_BUFFER at entry, install the preamble (Frame 1), replay preloaded snippets (Frame 3), execute their verb-specific logic, and write a single Outcome envelope (Result or Panic) to OUTCOME_BUFFER before returning. The host then reads the envelope via `__kobako_take_outcome` and applies the two-step attribution decision (`SPEC.md` § Behavior; `docs/behavior.md` § Error Scenarios).

The Host Gem calls `__kobako_yield_to_block` from inside a `__kobako_dispatch` callback when the Service method invokes its yield proxy (B-24). The host writes the yield arguments as a MessagePack payload (an array of positional args) into linear memory at `[req_ptr, req_ptr + req_len)`. The Guest Binary executes the block body within the active dispatch frame, allocates a response buffer via `__kobako_alloc`, writes the YieldResponse bytes (→ YieldResponse Envelope), and returns the packed i64. The single-RPC 16 MiB payload size limit applies in both directions.

### Invocation channels

Each invocation entry point consumes a fixed sequence of inputs across two host→guest channels: WASI stdin (length-prefixed frames `[u32 be][bytes]`) and the command buffer (msgpack at `(ptr, len)` reachable via `__kobako_alloc` plus a linear-memory write, then surfaced as typed export arguments).

| Export | WASI stdin frames | Command buffer |
|---|---|---|
| `__kobako_eval` | Frame 1 preamble · Frame 2 user source · Frame 3 snippets | — |
| `__kobako_run` | Frame 1 preamble · Frame 3 snippets | invocation envelope at `(env_ptr, env_len)` |

Frame definitions:

- **Frame 1 — preamble**: msgpack-encoded Service registration table (Namespace / Member metadata). Always present.
- **Frame 2 — user source**: the `code` argument to `#eval`, as raw UTF-8 bytes. Read only by `__kobako_eval`. Loads with backtrace filename `(eval)`.
- **Frame 3 — snippets**: msgpack array of `{name, kind, body}` entries — one entry per snippet preloaded via `#preload` (B-32), in insertion order. **Mandatory-presence** even when empty: a Sandbox with no preloads sends an empty msgpack array, not an absent frame. The guest loads each entry with backtrace filename `(snippet:<name>)`. Loads execute in insertion order before per-invocation logic (user source for `__kobako_eval`; entrypoint resolution for `__kobako_run`). The `kind` field carries the legal string value `"source"` in this revision — the only kind of snippet a Sandbox may currently register. The field exists as a forward-compatibility slot so the upcoming bytecode preload path (`#preload(binary:)` and `Sandbox.compile`, out of scope for this revision) can introduce additional kinds without reshaping the wire envelope; until that revision lands, any value other than `"source"` is a wire violation.
- **Invocation envelope** (`__kobako_run` only): msgpack map carrying entrypoint name + positional args + keyword args. Exact shape:
  - `entrypoint`: Symbol (ext 0x00) — the host has already normalized any String input via `.to_sym`.
  - `args`: Array — zero or more positional argument values. Empty array is present, never absent. Handles (ext 0x01) are rejected at host pre-flight (E-29).
  - `kwargs`: Map — zero or more keyword argument entries. Empty map is present, never absent. Map keys are Symbol (ext 0x00) only; non-Symbol keys are rejected at host pre-flight (E-30).

Mandatory-presence frames (1 and 3 for `__kobako_eval`; 1 and 3 for `__kobako_run`) and explicit empty payloads remove the `read_exact` EOF / partial-read ambiguity from each export's per-invocation contract.

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

Memory ownership: all buffer pointers refer to wasm linear memory owned by the Guest Binary Wasm instance. The Host Gem reads through a memory view provided by the Wasm engine during the call frame. After the call frame exits, the Host Gem holds no references to guest memory. Buffers are not individually freed; the entire wasm linear memory is released when the Wasm instance is dropped at the end of the `#run` invocation.

---

## Consistency Guarantee

Round-trip fuzz is the sole mechanism by which Host Gem and Guest Binary codec implementations are verified to agree. The two sides implement the codec independently (in Ruby and in Rust/mruby respectively) with no shared codec source. The fuzz contract is bidirectional:

- **Host → Guest → Host**: Host Gem encodes a payload → Guest Binary decodes and re-encodes → Host Gem decodes → deep equality with original.
- **Guest → Host → Guest**: Guest Binary encodes a payload → Host Gem decodes and re-encodes → Guest Binary decodes → deep equality with original.

Both directions are required. Coverage must include all 12 wire types (→ Type Mapping), all three ext types (0x00 Symbol, 0x01 Capability Handle, 0x02 Fault envelope), and nested compositions (e.g., array of Handles, map with symbol keys, map containing bin values, Panic envelope with optional `details`). Any round-trip fuzz failure is a wire regression that blocks release. The harness contract for this fuzz layer is specified in `SPEC.md` § Implementation Standards → Testing Style.
