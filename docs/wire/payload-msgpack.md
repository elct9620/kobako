# MessagePack Payload Codec

This document pins the byte encoding of the **payload** — the opaque `bytes` field every core envelope hands through (→ [`envelope.md`](envelope.md)). MessagePack is kobako's default payload codec: it is what the bundled Guest Binary and the Ruby frontend speak, and what a host gets without choosing anything.

It is not the only codec the wire admits. The core envelope routes and attributes without reading a payload byte, so a host and guest that agree on another schema replace this document with their own and carry no MessagePack dependency. What is fixed for every codec is the envelope, not this encoding.

`docs/wire-codec.md` is the anchor that relates the two layers; this document is the default codec's byte-level reference. The abstract shape it encodes is specified in [`../wire-contract.md`](../wire-contract.md).

The Host Gem (`lib/kobako/`) and the Guest Binary (`crates/kobako-codec`) implement this codec independently in different languages; byte-level round-trips between them are pinned by the oracle fuzz checks (→ `docs/wire-codec.md` § Consistency Guarantee).

---

## Payload Positions

A codec owns exactly these positions. Everything else in a message belongs to the core envelope.

| Position | Content |
|----------|---------|
| Call `payload` | The invocation arguments — `args` (ordered list) and `kwargs` (Symbol-keyed map) |
| Reply `body`, `tag=0` | The Service method's return value |
| Yield Call | The block's yield arguments as an ordered list |
| Yield Reply `body`, `tag` `0x01` / `0x02` | The block's value, or the `break` value |
| Outcome `body`, `tag=0x01` | The invocation's value |
| Run `payload` | The entrypoint's `args` and `kwargs` |
| Frame 3 entry `body` | Not codec-encoded — raw UTF-8 source or RITE bytecode |

A payload is exactly one MessagePack value. Bytes remaining after that value are a wire violation; the receiving side rejects the payload instead of ignoring the excess.

### Call and Run payload shape

Both carry a 2-element MessagePack array with fixed positions:

| Index | Field | Type |
|-------|-------|------|
| 0 | `args` | array (elements may include ext 0x01 Handles) |
| 1 | `kwargs` | map (Symbol keys as ext 0x00; values may include ext 0x01 Handles) |

Both elements are always present. An empty `args` is the empty array; an empty `kwargs` is the empty map (`0x80`) — never absent, so field positions stay stable.

Positional-versus-keyword partition is a codec concern: a schema without Ruby's call semantics carries whatever shape its own language needs, and the core envelope is unchanged.

---

## Type Mapping

The following 11 entries constitute the complete set of MessagePack types this codec recognizes. Any msgpack type or ext code not listed here is a wire violation; both sides reject it without attempting to decode further.

| # | msgpack family | Wire use | Host Gem Ruby type | Guest Binary mruby / Rust type |
|---|----------------|----------|--------------------|-------------------------------|
| 1 | nil | Absent optional fields; explicit `nil` values | `nil` | `nil` (mruby) / `Option::None` |
| 2 | bool | Boolean values | `true` / `false` | `TrueClass` / `FalseClass` (mruby) / `bool` |
| 3 | int (all widths: fixint, int 8/16/32/64, uint 8/16/32/64) | Integer values | `Integer` | `Integer` (mruby) / `i64` or `u64` |
| 4 | float (float 32 / float 64) | Floating-point values | `Float` | `Float` (mruby) / `f64` |
| 5 | str (fixstr / str 8 / str 16 / str 32) | UTF-8 text strings (see str/bin rules below) | `String` (UTF-8 encoding) | `String` (mruby) / `&str` / `String` |
| 6 | bin (bin 8 / bin 16 / bin 32) | Arbitrary byte sequences (see str/bin rules below) | `String` (binary / ASCII-8BIT encoding) | `String` (mruby, binary) / `&[u8]` / `Vec<u8>` |
| 7 | array (fixarray / array 16 / array 32) | Ordered sequences; the `args` / `kwargs` payload framing | `Array` | `Array` (mruby) / `Vec<T>` |
| 8 | map (fixmap / map 16 / map 32) | Associative maps; `kwargs` | `Hash` | `Hash` (mruby) / struct or `HashMap` |
| 9 | ext (general channel) | Dispatch point; this codec uses ext codes 0x00 and 0x01; all other ext codes are wire violations | — (dispatch by code) | — (dispatch by code) |
| 10 | ext 0x00 | Symbol (see Ext Types below) | `Symbol` | `Symbol` (mruby `mrb_sym`) / `Sym(String)` |
| 11 | ext 0x01 | Capability Handle (see Ext Types below) | `Kobako::Handle` | `Kobako::Handle` (mruby) / `Handle(u32)` |

---

## Integer Range

The Host Gem represents `Integer` at arbitrary precision; the Guest Binary represents it as a signed 32-bit value. An inbound integer outside the guest's signed 32-bit range therefore has no faithful guest representation. On every host→guest path — a `#run` / `#eval` argument, a yield-block argument, or a dispatch return value — the guest refuses such a value rather than saturating it to the nearest bound, so neither side ever sees a different number than the wire carried. The refusal travels each path the way that path already reports a malformed payload: a `#run` / `#eval` argument fails the invocation as a guest-entry envelope rejection (E-26); a yield-block argument fails the yield round-trip; and a dispatch return value raises in the guest code that made the call. The reverse direction never overflows: a guest `Integer` always fits the host's arbitrary-precision `Integer`.

---

## Text and Bytes

The Host Gem tags a `String` with an encoding; the Guest Binary's mruby `String` is a byte array carrying no encoding tag. So where the host chooses a family by what the value claims to be, the guest has only one rule available: bytes decide. A guest `String` whose bytes are valid UTF-8 rides as `str`, and any other byte sequence rides as `bin`. Both families are legal at every payload position a value reaches, so the bytes cross intact either way — the guest never renders a `String` into text it is not, which would answer with the bytes a `str` can hold and drop the rest in silence.

**The bytes are preserved; the tag is not.** A host `String` that rode out as `bin` and comes back through the guest arrives as `str` whenever its bytes happen to be valid UTF-8, because the guest had no tag to carry and re-derives one from the bytes. A value whose encoding matters carries it in the value, not in the family.

Choosing by tag lets the two disagree: a host `String` tagged UTF-8 whose bytes are not rides as `str`, which the family does not allow. The guest's decoder rejects it as the wire violation it is, so the invocation fails the way that path already fails on a malformed payload rather than delivering bytes under a family that promises otherwise.

A `Symbol` has no second family: its name rides as ext 0x00, which requires UTF-8. A guest `Symbol` whose name is not UTF-8 therefore has no wire representation at all, and the guest refuses it on each guest→host path the way that path already refuses an unrepresentable value — a return value as E-06, a dispatch argument or keyword name as E-55, a yield-block result as E-22 (→ [`../behavior/errors.md`](../behavior/errors.md)). The reverse direction does not arise: a host `Symbol` is UTF-8 by construction.

---

## Structural Nesting Depth

Encoded values nest to at most 128 levels — the MessagePack ecosystem's established limit.

**The budget is per document.** The core envelope carries no nesting of its own (→ [`envelope.md`](envelope.md) § Size and Depth Bounds), and each payload it hands through is decoded as its own document with its own 128-level budget.

Every decoder enforces the bound: the Host Gem's codec library on its decode path, and the Guest Binary's decoder on every inbound payload, so a host→guest value nesting deeper than the bound fails as a clean wire error rather than overflowing the wasm stack. The Guest Binary encoder caps its recursive walk at the same depth: a guest return or yield-block result nesting deeper than the bound — which a reference cycle necessarily does — has no wire representation and surfaces as E-06 / E-22 (→ [`../behavior/errors.md`](../behavior/errors.md)) rather than a hard trap. The host rejects a `#run` argument nesting deeper — as a reference cycle necessarily does — while encoding the payload, raising `Kobako::SandboxError` (E-54). The guest likewise rejects a dispatch argument nesting deeper — and, more broadly, any dispatch argument or kwargs value outside the wire type set — at the dispatch call site rather than coercing it to a string, surfacing as E-55. The guest cap and the host library's limit sit at the same depth; a value right at the boundary is rejected as a clean error by whichever side reaches its limit first, never as a trap.

---

## str / bin Encoding Rules

msgpack distinguishes `str` (UTF-8 text) from `bin` (raw bytes). The following rules govern which family is used at each payload position. A violation of a "str only" rule is a wire violation and the receiving side rejects the payload.

| Payload position | Accepted family | Violation handling |
|---|---|---|
| `args` elements and `kwargs` values | str or bin (context-determined) | both are legal |
| Reply ok body, Yield Reply ok / break body, Outcome result body | str or bin (context-determined) | both are legal |

The core envelope's own text fields — `target`, `method`, `entrypoint`, `origin`, `class`, `message`, backtrace lines, snippet names — are length-prefixed UTF-8 byte strings at that layer and never reach this codec (→ [`envelope.md`](envelope.md)).

Symbols travel as ext 0x00. A Symbol encoded on one side and decoded on the other arrives as a Symbol with the same UTF-8 name; symbol identity across the wire is established by name equality, not by interned-id sharing. A `str` or `bin` value carrying the bytes of a symbol name is **not** wire-equivalent to that Symbol; the two are distinguishable on the wire and must remain distinguishable end-to-end.

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

The payload bytes MUST decode as UTF-8. A non-UTF-8 payload is a wire violation: encoders MUST validate UTF-8 before emitting — a name that fails leaves the Symbol with no representation, not with a substitute one (→ § Text and Bytes) — and decoders MUST reject the payload rather than fall back to a binary-encoded Symbol. The payload length is bounded only by msgpack's natural ext-family limits; kobako does not impose an additional cap.

Position rules for ext 0x00:

- **MUST be ext 0x00** at: `kwargs` map keys (no other wire type is accepted at this position; a `str`, `bin`, or other-type key is a wire violation).
- **MAY appear** at: `args` elements, `kwargs` values, any value payload, and as elements / keys / values of any nested array or map within those positions (other wire types are also permitted).

### ext 0x01 — Capability Handle

**Binary layout:** fixed 4-byte payload, big-endian u32 Handle ID. The msgpack framing is `fixext 4`: format byte `0xd6`, type byte `0x01`, followed by 4 bytes of big-endian u32 data. Total wire size: 6 bytes.

| Byte offset | Content |
|-------------|---------|
| 0 | `0xd6` — msgpack `fixext 4` marker |
| 1 | `0x01` — kobako ext type code |
| 2–5 | Handle ID as big-endian u32 |

The Handle ID field carries the opaque identifier allocated by `Catalog::Handles` (→ [`../wire-contract.md`](../wire-contract.md) § Capability Handle). ID 0 is reserved as the invalid sentinel. The maximum valid ID is `0x7fff_ffff` (2³¹ − 1); any ID above this cap is a wire violation.

ext 0x01 may appear in any payload position, at any nesting depth, in both directions: `args` elements and `kwargs` values of a Call or a Run alike, a Reply's success value, the Outcome's value, and a Yield Reply's ok or break value. Run payload positions carry Handles produced by host-side auto-wrap (→ [`../behavior/dispatch.md`](../behavior/dispatch.md) § B-34); the framing and ID semantics are identical in every position.

**A Handle in the `target` position is a core-envelope field, not an ext value** (→ [`envelope.md`](envelope.md) § Call): the envelope's `kind` byte carries the discrimination and the ID rides as a bare `u32`. A codec that carries no Handle representation at all still reaches a Handle target — which is the common case for a stateful receiver — and only forgoes passing Handles as arguments.

A Fault has no ext code here. Every byte of one is kobako's — a closed category and a message — so it rides the envelope's own fault arm (→ [`envelope.md`](envelope.md) § Fault), where a guest reads it with no codec at all and a replacement codec owes it nothing.
