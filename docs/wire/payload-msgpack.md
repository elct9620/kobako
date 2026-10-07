# MessagePack Payload Codec

This document pins the bytes of the payload, the opaque field every core envelope hands through. MessagePack is the default codec: the bundled Guest Binary and the Ruby frontend speak it.

| Document | Holds |
|----------|-------|
| [`README.md`](README.md) | the two layers, and how the peers are held consistent |
| [`envelope.md`](envelope.md) | the envelope that carries each payload |
| [`spec/behavior/payload-encoding.md`](../spec/behavior/payload-encoding.md) | this encoding's behavior |

The Host Gem (`lib/kobako/`) and the guest (`kobako-codec`) implement it independently, in different languages. Another codec may replace it, since the envelope never reads a payload byte.

---

## Payload Positions

A codec owns exactly these positions; everything else in a message is the envelope's. A payload is exactly one MessagePack value.

| Position | Content |
|----------|---------|
| Call `payload` | `args` and `kwargs` |
| Reply `body`, success | the Service method's return value |
| Yield Call | the yield arguments, as a list |
| Yield Reply `body`, ok or break | the block's value, or the `break` value |
| Outcome `body`, ok | the invocation's value |
| Run `payload` | the entrypoint's `args` and `kwargs` |

The envelope's own text fields, such as `method` or `origin`, never reach this codec. A snippet frame's body is raw source or bytecode, not a payload.

### Argument Shape

A Call and a Run payload are a 2-element array with fixed positions.

| Index | Field | Type |
|-------|-------|------|
| 0 | `args` | array; elements may be Handles |
| 1 | `kwargs` | map; Symbol keys, values may be Handles |

Both elements are always present, an empty `kwargs` being the empty map (`0x80`), so positions stay stable. The positional-versus-keyword split is this codec's; another schema carries whatever shape its language needs.

---

## Type Mapping

These 11 types are the complete, closed set this codec carries ([payload-encoding](../spec/behavior/payload-encoding.md)). Each row is one MessagePack family and what each side holds it as.

| msgpack family | Host Ruby | Guest mruby | Guest `Value` |
|----------------|-----------|-------------|---------------|
| nil | `nil` | `nil` | `Nil` |
| bool | `true` / `false` | `true` / `false` | `Bool` |
| int, uint | `Integer` | `Integer` | `Int` / `UInt` |
| float 32 / 64 | `Float` | `Float` | `Float` |
| str | `String`, UTF-8 | `String` | `Str` |
| bin | `String`, ASCII-8BIT | `String` | `Bin` |
| array | `Array` | `Array` | `Array` |
| map | `Hash` | `Hash` | `Map`, ordered pairs |
| ext 0x00 | `Symbol` | `Symbol` | `Sym` |
| ext 0x01 | `Kobako::Handle` | `Kobako::Handle` | `Handle` |

Any other ext code is refused. `Guest Value` is `kobako_codec::msgpack::Value`, the guest codec's decoded form.

---

## Integer Range

The host's `Integer` is arbitrary precision; the guest's is signed 32-bit. Only the host→guest direction can therefore overflow.

| Side | `Integer` width |
|------|-----------------|
| Host Gem | arbitrary precision |
| Guest Binary | signed 32-bit |

The guest refuses an inbound integer outside its range rather than saturating it, so neither side sees a number the wire did not carry. This holds on every host→guest path: a `#run` argument, a block argument, and a dispatch return value. Each reports the refusal as it reports a malformed payload ([codec](../spec/behavior/codec.md)).

---

## Text and Bytes

The host tags a `String` with an encoding; a guest `String` is untagged bytes. So each side picks the family by what it has.

| Value | Rides as |
|-------|----------|
| host `String` tagged binary | `bin` |
| any other host `String` | `str` |
| guest `String` of valid UTF-8 | `str` |
| any other guest `String` | `bin` |
| `Symbol` | ext 0x00, UTF-8 only |

Both families are legal at every value position, so the bytes always cross intact, but the tag does not. A value whose encoding matters carries it in the value. A host `String` whose tag claims text it does not hold, and a guest `Symbol` whose name is not UTF-8, have no representation ([codec](../spec/behavior/codec.md)).

---

## Structural Nesting Depth

Encoded values nest at most 128 levels, the host MessagePack library's limit, so both sides refuse at the same depth. Each payload has its own budget; the envelope needs none.

| Where | Enforced by |
|-------|-------------|
| every decode | each side's decoder |
| guest return, yield result, dispatch argument | the guest encoder's capped walk |
| `#run` argument | the host, while encoding |
| Service answer, yield arguments | the host, measured before encoding |

A value past the bound, a reference cycle included, fails as a clean error, never a trap ([codec](../spec/behavior/codec.md)). The host library has no encoder depth limit, so the host measures its outbound positions itself. That refusal lands where the Service hands the value over, since only the Service can change it.

---

## Ext Types

kobako uses two ext codes. A Fault has none: it rides the envelope's own arm (→ [`envelope.md`](envelope.md) § Fault), so a replacement codec owes it nothing.

### ext 0x00 — Symbol

A Symbol's ext payload is its UTF-8 name. MessagePack picks the frame by the name's length, so the type code's offset varies.

| Name length | Frame | Header before the name |
|-------------|-------|------------------------|
| 1, 2, 4, 8, 16 | fixext (`0xd4`–`0xd8`) | marker, `0x00` |
| 0, or 3–255 otherwise | ext 8 (`0xc7`) | marker, 1-byte length, `0x00` |
| 256–65535 | ext 16 (`0xc8`) | marker, 2-byte length, `0x00` |
| larger | ext 32 (`0xc9`) | marker, 4-byte length, `0x00` |

The empty Symbol `:""` is `0xc7 0x00 0x00`. A `kwargs` key must be a Symbol; any other value position may hold one. Identity across the wire is by name, and a Symbol stays distinct from text spelling its name ([payload-encoding](../spec/behavior/payload-encoding.md)).

### ext 0x01 — Handle

A Handle is a `fixext 4`, six bytes in all. Its ID follows [`README.md`](README.md) § Capability Handle.

| Byte offset | Content |
|-------------|---------|
| 0 | `0xd6`, the `fixext 4` marker |
| 1 | `0x01`, the kobako ext code |
| 2–5 | the Handle ID, big-endian u32 |

A Handle may appear at any payload position and depth, in both directions; in a Run payload it comes from host-side auto-wrap. A codec without a Handle representation is legal, and forgoes only Handles as values.
