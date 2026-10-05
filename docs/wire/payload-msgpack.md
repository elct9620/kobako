# MessagePack Payload Codec

This document pins the byte encoding of the payload, the opaque `bytes` field every core envelope hands through. MessagePack is kobako's default payload codec: the bundled Guest Binary and the Ruby frontend speak it.

| Document | Holds |
|----------|-------|
| [`../wire-codec.md`](../wire-codec.md) | how the two layers relate; the fuzz checks |
| [`envelope.md`](envelope.md) | the envelope that carries each payload |
| [`../wire-contract.md`](../wire-contract.md) | the abstract shape encoded here |
| [`../spec/behavior/payload-encoding.md`](../spec/behavior/payload-encoding.md) | this encoding's behavior |

Another codec may replace this one, since the envelope never reads a payload byte. The Host Gem (`lib/kobako/`) and the Guest Binary (`crates/kobako-codec`) implement it independently, in different languages.

---

## Payload Positions

A codec owns exactly these positions. Everything else in a message belongs to the core envelope.

| Position | Content |
|----------|---------|
| Call `payload` | `args` (ordered list) and `kwargs` (Symbol-keyed map) |
| Reply `body`, `tag=0` | the Service method's return value |
| Yield Call | the block's yield arguments as an ordered list |
| Yield Reply `body`, `tag` `0x01` / `0x02` | the block's value, or the `break` value |
| Outcome `body`, `tag=0x01` | the invocation's value |
| Run `payload` | the entrypoint's `args` and `kwargs` |
| Frame 3 entry `body` | not codec-encoded: raw source or RITE bytecode |

A payload is exactly one MessagePack value. The envelope's own text fields, such as `method` or `origin`, never reach this codec.

### Argument Shape

Both carry a 2-element MessagePack array with fixed positions.

| Index | Field | Type |
|-------|-------|------|
| 0 | `args` | array; elements may be ext 0x01 Handles |
| 1 | `kwargs` | map; Symbol keys as ext 0x00, values may be Handles |

Both elements are always present, an empty `kwargs` being the empty map (`0x80`), so positions stay stable. The positional-versus-keyword split is this codec's concern; another schema carries whatever shape its language needs.

---

## Type Mapping

These 11 entries are the complete, closed set of types this codec recognizes (→ [`../spec/behavior/payload-encoding.md`](../spec/behavior/payload-encoding.md)).

| msgpack family | Wire use | Host Gem Ruby type | Guest mruby / Rust type |
|----------------|----------|--------------------|-------------------------|
| nil | absent or explicit `nil` | `nil` | `nil` / `Option::None` |
| bool | booleans | `true` / `false` | `TrueClass`, `FalseClass` / `bool` |
| int (fixint, int 8–64, uint 8–64) | integers | `Integer` | `Integer` / `i64` or `u64` |
| float (32 / 64) | floating point | `Float` | `Float` / `f64` |
| str (fixstr, str 8/16/32) | UTF-8 text | `String` (UTF-8) | `String` / `&str`, `String` |
| bin (bin 8/16/32) | arbitrary bytes | `String` (ASCII-8BIT) | binary `String` / `&[u8]`, `Vec<u8>` |
| array (fixarray, array 16/32) | sequences; argument framing | `Array` | `Array` / `Vec<T>` |
| map (fixmap, map 16/32) | maps; `kwargs` | `Hash` | `Hash` / struct or `HashMap` |
| ext (general channel) | dispatch by code; only 0x00 and 0x01 | — | — |
| ext 0x00 | Symbol (§ Ext Types) | `Symbol` | `Symbol` (`mrb_sym`) / `Sym(String)` |
| ext 0x01 | Capability Handle (§ Ext Types) | `Kobako::Handle` | `Kobako::Handle` / `Handle(u32)` |

---

## Integer Range

The two sides represent `Integer` at different widths, so only the host→guest direction can overflow.

| Side | `Integer` width |
|------|-----------------|
| Host Gem | arbitrary precision |
| Guest Binary | signed 32-bit |

The guest refuses an inbound integer outside its range rather than saturating it, so neither side sees a number the wire did not carry. This holds on every host→guest path: a `#run` argument, a yield-block argument, and a dispatch return value. Each path reports the refusal the way it reports a malformed payload (→ [`../spec/behavior/codec.md`](../spec/behavior/codec.md)).

---

## Text and Bytes

The host tags a `String` with an encoding; a guest mruby `String` is bytes with no tag. So each side picks the family by what it has.

| Value | Rides as |
|-------|----------|
| host `String` tagged binary | `bin` |
| any other host `String` | `str` |
| guest `String` of valid UTF-8 | `str` |
| any other guest `String` | `bin` |
| `Symbol` | ext 0x00, UTF-8 only |

Both families are legal at every value position, so the bytes always cross intact. The tag is not preserved: a value whose encoding matters carries it in the value. A host `String` whose tag claims text it does not hold, and a guest `Symbol` whose name is not UTF-8, have no representation (→ [`../spec/behavior/codec.md`](../spec/behavior/codec.md)).

---

## Structural Nesting Depth

Encoded values nest to at most 128 levels, the MessagePack ecosystem's established limit. The budget is per document: each payload is decoded with its own budget (→ [`envelope.md`](envelope.md) § Size and Depth Bounds).

| Where | Enforced by |
|-------|-------------|
| every decode | each side's decoder |
| guest return, yield result, dispatch argument | the Guest Binary encoder's capped walk |
| `#run` argument | the host, while encoding the payload |
| Service answer, yield arguments | the host, measured before its library writes |

Wherever a bound is carried, both sides sit at the same depth. A value past it, a reference cycle included, fails as a clean error rather than a trap (→ [`../spec/behavior/codec.md`](../spec/behavior/codec.md)).

The host's codec library has no encoder depth limit and no stack guard, so the host measures the two outbound dispatch positions itself. The refusal lands where the Service hands the value over, since only the Service can change it.

---

## Ext Types

### ext 0x00 — Symbol

A Symbol is a variable-length ext whose payload is its UTF-8 name, framed `ext 8` or `ext 16` by size.

| Byte offset | Content |
|-------------|---------|
| 0 | `0xc7` or `0xc8` — msgpack `ext 8` / `ext 16` marker |
| 1 | length: 1 byte for `ext 8`, 2 big-endian bytes for `ext 16` |
| n | `0x00` — kobako ext type code |
| n+1.. | UTF-8 bytes of the symbol name |

| Position | Rule |
|----------|------|
| `kwargs` map keys | must be ext 0x00 |
| any other value position, at any depth | may be ext 0x00 |

An empty payload (`0xc7 0x00 0x00`) is the empty Symbol `:""`. The length has no cap beyond msgpack's own. Identity across the wire is by name, and a Symbol stays distinct from text spelling its name (→ [`../spec/behavior/payload-encoding.md`](../spec/behavior/payload-encoding.md)).

### ext 0x01 — Capability Handle

A Handle is a `fixext 4`: format byte `0xd6`, type byte `0x01`, then a big-endian u32 Handle ID, 6 bytes in all.

| Byte offset | Content |
|-------------|---------|
| 0 | `0xd6` — msgpack `fixext 4` marker |
| 1 | `0x01` — kobako ext type code |
| 2–5 | Handle ID as big-endian u32 |

The ID is the opaque identifier `Catalog::Handles` allocates (→ [`../wire-contract.md`](../wire-contract.md) § Capability Handle). ID `0` is the invalid sentinel, and `0x7fff_ffff` is the maximum. A Handle may appear at any payload position and depth, in both directions; in a Run payload it comes from host-side auto-wrap.

A Handle in the `target` position is an envelope field, not an ext value (→ [`envelope.md`](envelope.md) § Call). A codec without a Handle representation is legal: it still reaches a Handle target and forgoes only Handles as arguments or values.

A Fault has no ext code here. It rides the envelope's own fault arm (→ [`envelope.md`](envelope.md) § Fault), so a replacement codec owes it nothing.
