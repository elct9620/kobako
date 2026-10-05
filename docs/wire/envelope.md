# Core Envelope

This document pins the byte layout of the core envelope, the outer frame of every host↔guest message. It is a fixed layout, not MessagePack, and hands everything but routing and attribution through as an opaque `payload`.

| Document | Holds |
|----------|-------|
| [`../wire-codec.md`](../wire-codec.md) | the ABI surface, and how the two layers relate |
| [`../wire-contract.md`](../wire-contract.md) | the abstract shape encoded here |
| [`payload-msgpack.md`](payload-msgpack.md) | the default payload codec |
| [`../spec/behavior/envelope.md`](../spec/behavior/envelope.md) | the envelope's behavior |

`crates/kobako-transport` implements this layout once, for both sides. Its golden vectors derive from this document, not from that code (→ `docs/wire-codec.md` § Consistency Guarantee).

---

## Properties of this layer

Every layout below satisfies both properties.

| Property | Consequence |
|----------|-------------|
| Decodable without the payload codec | the codec is replaceable; a protobuf pair needs no MessagePack |
| Non-recursive | no nesting to bound; no stack to overflow on untrusted input |

Every field is a scalar, a byte string, or a flat list. A side routes a Call and attributes a failure from these fields alone.

---

## Primitives

Every core envelope is built from four primitives. All integers are unsigned big-endian, matching the invocation-channel frame prefix.

| Primitive | Layout |
|-----------|--------|
| `u8` | one byte |
| `u32` | four bytes, big-endian |
| `bytes` | `u32` length, then exactly that many bytes |
| `list<bytes>` | `u32` count, then that many `bytes` values back to back |

A `bytes` field of length `0` is a present, empty value. A field that can mean "absent" says so in its table row.

### Framing rule

Every field except the last is self-delimiting; the last field consumes the remainder of the message.

```
[ field ][ field ] ... [ last field ............ ]
 self-delimiting        remainder of the message
```

The last field is the `payload` or `body`, whose length the transport already knows from the frame prefix or the ABI's `len`. Repeating it inside the envelope would give two sources for one fact. An envelope nested as another's trailing field inherits the remainder as its own extent.

A decode consumes the message exactly or fails, so a framing desync fails loudly (→ [`../spec/behavior/envelope.md`](../spec/behavior/envelope.md)).

---

## Call

The guest→host dispatch Call, and the shape every reverse-direction Call is measured against.

| Field | Type | Meaning |
|-------|------|---------|
| `kind` | `u8` | `0` constant path · `1` Capability Handle reference |
| `target` | `bytes` when `kind=0`, `u32` when `kind=1` | UTF-8 path (`"MyService::KV"`), or the Handle ID |
| `method` | `bytes` | the method name as UTF-8; one per Call |
| `block_given` | `u8` | `0` or `1` |
| `payload` | remainder | the arguments, codec-encoded; opaque here |

The explicit `kind` tag discriminates the two `target` forms, so a side reads routing fields without consulting any other encoding. Handle ID `0` is the invalid sentinel and `0x7fff_ffff` the maximum (→ [`../wire-contract.md`](../wire-contract.md) § Capability Handle).

---

## Reply

The answer to one dispatch Call.

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | `0` success · `1` fault |
| `body` | remainder | `tag=0`: the codec-encoded value · `tag=1`: a Fault |

Success-versus-fault is decided at this layer, so a guest learns whether the Service returned or raised from one byte, whatever the payload schema. That is why the fault rides its own arm rather than a reserved payload value.

### Fault

The host refusing or failing a Call. Every byte is kobako's, so it rides the envelope and a guest reads it with no codec.

| Field | Type | Meaning |
|-------|------|---------|
| `kind` | `u8` | `0` runtime · `1` argument · `2` undefined · `3` internal · `4` block |
| `message` | `bytes` | human-readable description as UTF-8 |

The category is a tag, so both sides spell it alike without agreeing on text. The values keep their meanings from [`../wire-contract.md`](../wire-contract.md) § Fault. An unknown kind or trailing field degrades rather than fails, which keeps an addition survivable by an older peer. An unknown kind degrades to `undefined`, which never claims the Service ran.

A Fault carries no backtrace. A host backtrace names paths and code the guest cannot see, which the boundary cannot bound. That is why a Fault and an Error Record stay separate types.

---

## Yield Call and Yield Reply

The reverse-direction pair, nested inside the dispatch frame the host is still answering.

| Message | Field | Type | Meaning |
|---------|-------|------|---------|
| Yield Call | payload | whole message | the yield arguments, codec-encoded |
| Yield Reply | `tag` | `u8` | `0x01` ok · `0x02` break · `0x04` error |
| Yield Reply | `body` | remainder | ok or break value, codec-encoded · or an Error Record |

The ABI's `req_len` frames a Yield Call, so no length prefix repeats (→ `docs/wire-codec.md` § ABI Signatures). Tag `0x03` is reserved. An answer outside the live tags is refused (→ [`../spec/behavior/transport-yield.md`](../spec/behavior/transport-yield.md)).

### Error Record

The guest's report that something it ran raised. Block and invocation failures share it, and the host re-raises from these fields with no codec.

| Field | Type | Meaning |
|-------|------|---------|
| `name` | `bytes` | the error's name as UTF-8 (`"RuntimeError"`) |
| `message` | `bytes` | human-readable description as UTF-8 |
| `backtrace` | `list<bytes>` | mruby backtrace, one UTF-8 line each; may be empty |

It differs from a Fault, which travels host to guest with a category instead of an error's own name.

---

## Outcome

The per-invocation result, written to OUTCOME_BUFFER and read by the host through `__kobako_take_outcome`.

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | `0x01` ok · `0x02` Panic |
| `body` | remainder | `tag=0x01`: the codec-encoded value · `tag=0x02`: a Panic |

The ok body is the value alone, since the `tag` already discriminates. An empty buffer or unknown tag is refused (→ [`../spec/behavior/outcome.md`](../spec/behavior/outcome.md)).

### Panic

The Error Record plus the fields attribution and correction need.

| Field | Type | Meaning |
|-------|------|---------|
| `origin` | `bytes` | `"sandbox"` or `"service"` as UTF-8 |
| `name` | `bytes` | the error's name as UTF-8 |
| `message` | `bytes` | exception message as UTF-8 |
| `backtrace` | `list<bytes>` | mruby backtrace, one UTF-8 line each |
| `available` | `list<bytes>` | names the failed `#run` entrypoint could have used; may be empty |

Panic carries no codec-encoded field. A host attributes a failure from `origin` and reports its correction from `available` without decoding a payload byte (→ [`../spec/behavior/outcome.md`](../spec/behavior/outcome.md)).

---

## Run

The host→guest entrypoint dispatch, delivered on the command buffer to `__kobako_run`.

| Field | Type | Meaning |
|-------|------|---------|
| `entrypoint` | `bytes` | top-level constant name as UTF-8, `/\A[A-Z]\w*\z/` |
| `payload` | remainder | the entrypoint's arguments, codec-encoded |

Run is the reverse-direction sibling of Call. It carries no `method`, since the entrypoint is invoked through `#call`, and no `block_given`, since `#run` supplies no block.

---

## Invocation Frames

The stdin frames every invocation entry point reads (→ `docs/wire-codec.md` § Invocation channels). Each frame's `[u32 be][bytes]` prefix is the transport's, not part of these layouts.

| Frame | Content | Layout |
|-------|---------|--------|
| Frame 1 | preamble | below |
| Frame 2 | `#eval` user source | raw UTF-8, no envelope |
| Frame 3 | snippets | below |

### Frame 1 — preamble

The preamble names every bound constant.

| Field | Type | Meaning |
|-------|------|---------|
| `paths` | `list<bytes>` | each bound constant's path as UTF-8; empty when nothing is bound |

### Frame 3 — snippets

The snippet table replays in insertion order.

| Field | Type | Meaning |
|-------|------|---------|
| `count` | `u32` | number of entries that follow |
| per entry `kind` | `u8` | `0` mruby source · `1` RITE bytecode |
| per entry `name` | `bytes` | only when `kind=0`; backtraces show `(snippet:<name>)` |
| per entry `body` | `bytes` | UTF-8 source when `kind=0`; RITE bytecode when `kind=1` |

A bytecode entry carries no `name`, because its filename comes from the bytecode's own `debug_info` section (→ [`../spec/behavior/sandbox.md`](../spec/behavior/sandbox.md)).

---

## Size and Depth Bounds

A side checks the size bound before allocating, so an oversized message is a wire violation rather than an allocation to survive.

| Bound | Applies to | Owner |
|-------|------------|-------|
| 16 MiB per message | the whole envelope, either direction | this layer |
| same 16 MiB | invocation-channel frames too | this layer |
| nesting depth | each payload, per document | payload codec |

The frames cross the same boundary under the same prefix, so a receiver cannot treat them differently. A `memory_limit` below 16 MiB binds first, since the guest grows linear memory to hold what it reads.

This layer is non-recursive (→ § Properties of this layer), so it has no depth to budget. The envelope and each payload are separate documents with separate budgets (→ [`payload-msgpack.md`](payload-msgpack.md) § Structural Nesting Depth).
