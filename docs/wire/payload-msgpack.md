# MessagePack Payload Codec

This document pins the bytes of the payload, the opaque field every Envelope hands through. MessagePack is the default Codec: the bundled Guest Binary and both Frontends speak it.

| Document | Holds |
|----------|-------|
| [`README.md`](README.md) | the two layers, and how the peers are held consistent |
| [`envelope.md`](envelope.md) | the envelope that carries each payload |
| [`spec/behavior/payload-encoding.md`](../spec/behavior/payload-encoding.md) | this encoding's behavior |

Two peers implement it independently, in different languages: the Ruby gem (`lib/kobako/`) and `kobako-codec`, which serves both the Guest Binary and the Rust SDK. Another Codec may replace it, since the Envelope never reads a payload byte.

---

## Payload Positions

A Codec owns exactly these positions; everything else in a message is the Envelope's. A payload is exactly one MessagePack value.

| Position | Content |
|----------|---------|
| Call `payload` | `args` and `kwargs` |
| Reply `body`, success | the Service method's return value |
| Yield Call | the yield arguments, as a list |
| Yield Reply `body`, ok | the block's value |
| Outcome `body`, ok | the Invocation's value |
| Run `payload` | the entrypoint's `args` and `kwargs` |

The Envelope's own text fields, such as `method` or `origin`, never reach this Codec. A Preamble snippet's body is raw source or bytecode, not a payload.

### Argument Shape

A Call and a Run payload are a 2-element array with fixed positions.

| Index | Field | Type |
|-------|-------|------|
| 0 | `args` | array; elements may be Handles |
| 1 | `kwargs` | map; Symbol keys, values may be Handles |

Both elements are always present, an empty `kwargs` being the empty map (`0x80`), so positions stay stable. The positional-versus-keyword split is this Codec's; another Codec carries whatever shape its language needs.

---

## Type Mapping

These 11 types are the complete, closed set this Codec carries ([payload-encoding](../spec/behavior/payload-encoding.md)). Each row is one type and what each side holds it as.

| msgpack type | Host Ruby | Guest mruby | `Value` |
|--------------|-----------|-------------|---------|
| nil | `nil` | `nil` | `Nil` |
| bool | `true` / `false` | `true` / `false` | `Bool` |
| int | `Integer` | `Integer` | `Int` |
| uint | `Integer` | `Integer` | `UInt` |
| float 32 / 64 | `Float` | `Float` | `Float` |
| str | `String`, UTF-8 | `String` | `Str` |
| bin | `String`, ASCII-8BIT | `String` | `Bin` |
| array | `Array` | `Array` | `Array` |
| map | `Hash` | `Hash` | `Map`, ordered pairs |
| ext 0x00 | `Symbol` | `Symbol` | `Sym` |
| ext 0x01 | `Kobako::Handle` | `Kobako::Handle` | `Handle` |

Any other ext code is refused. `Value` is `kobako_codec::msgpack::Value`, the decoded form the Guest Binary and the Rust SDK share; the Rust SDK re-exports it as `kobako::msgpack::Value`.

---

## Integer Range

Both sides hold an `Integer` of arbitrary precision, and one crosses the wire within the range msgpack's integer types encode.

```
-2^63 <= wire Integer <= 2^64 - 1
```

Every integer the wire carries fits both sides, so neither refuses one inbound. An `Integer` past the range has no wire form, and the side holding it refuses to send it ([codec](../spec/behavior/codec.md)). A guest `Integer` within the range crosses whichever form mruby keeps it in.

---

## Text and Bytes

The host tags a `String` with an encoding; a guest `String` is untagged bytes. So each side picks the type by what it has.

| Value | Rides as |
|-------|----------|
| host `String` tagged binary | `bin` |
| any other host `String` | `str` |
| guest `String` of valid UTF-8 | `str` |
| any other guest `String` | `bin` |
| `Symbol` | ext 0x00, UTF-8 only |

Both types are legal at every value position, so the bytes always cross intact, but the tag does not. A value whose encoding matters carries it in the value. A host `String` whose tag claims text it does not hold, and a guest `Symbol` whose name is not UTF-8, have no representation ([codec](../spec/behavior/codec.md)).

---

## Structural Nesting Depth

Encoded values nest at most 128 levels, the host MessagePack library's limit, so both sides refuse at the same depth. Each payload has its own budget; the Envelope needs none. Every decode refuses a value past the bound, and every side refuses to send one.

| Value past the bound | Fails as |
|----------------------|----------|
| guest return value | a Sandbox failure |
| dispatch argument | an error the guest can rescue at its call |
| block result | the guest's `TypeError`, raised in the Service's yield |
| `#run` argument | a Sandbox failure, before the guest runs |
| Service answer, yield arguments | the Service's failure |

A reference cycle counts as past the bound, and each case is a clean error, never a Trap ([codec](../spec/behavior/codec.md)). A refusal of what a Service sends is the Service's failure, since only the Service can change that value.

---

## Value Budget

A payload the guest sends holds at most one value for each 16 bytes of the Invocation's memory cap, the least one guest value occupies. Every array, map, key, and element counts as one value. With no memory cap there is no budget.

```
values in a guest payload <= memory_limit / 16
```

The host refuses a payload past the budget as it refuses one nested too deep. What a guest makes the host build then stays in proportion to what the guest could hold, however it shares values. A payload the host sends needs no budget, since the guest pays for it in its own memory.

---

## Ext Types

kobako uses two ext codes. A Fault has none: it rides the Envelope's own arm (→ [`envelope.md`](envelope.md) § Fault), so a replacement Codec owes it nothing.

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

A Handle may appear at any payload position and depth, in both directions. It stands for a host object the Codec cannot carry by value, a `#run` argument included. A Codec without a Handle representation is legal, and forgoes only Handles as values.
