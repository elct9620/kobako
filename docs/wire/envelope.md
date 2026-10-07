# Core Envelope

This document pins the core envelope, the outer frame of every host↔guest message: what each field means and how it is laid out in bytes. It is a fixed layout, not MessagePack, and hands everything but routing and attribution through as an opaque `payload`.

| Document | Holds |
|----------|-------|
| [`README.md`](README.md) | how the two layers relate, the transport roles, the Capability Handle |
| [`abi.md`](abi.md) | the ABI surface that carries each message |
| [`payload-msgpack.md`](payload-msgpack.md) | the default payload codec |
| [`../spec/behavior/envelope.md`](../spec/behavior/envelope.md) | the envelope's behavior |

`crates/kobako-transport` implements this layout once, for both sides. Its golden vectors derive from this document, not from that code (→ [`README.md`](README.md) § Consistency Guarantee).

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

The guest→host dispatch Call, and the shape every reverse-direction Call is measured against. A Call separates the envelope fields that **route** it from the payload that **feeds** the method.

| Field | Type | Meaning |
|-------|------|---------|
| `kind` | `u8` | `0` constant path · `1` Capability Handle reference |
| `target` | `bytes` when `kind=0`, `u32` when `kind=1` | UTF-8 path (`"MyService::KV"`, `"File"`), or the Handle ID |
| `method` | `bytes` | the method name as UTF-8; one per Call, no multi-segment traversal |
| `block_given` | `u8` | `0` or `1`: whether the call site supplied a block |
| `payload` | remainder | positional and keyword arguments, Handles allowed; codec-encoded, opaque here |

The explicit `kind` tag discriminates the two `target` forms, so a side reads routing fields without consulting `method` or any other encoding. The string form uses Ruby constant-path syntax, so the wire value matches the guest's own constant access. Handle ID `0` is the invalid sentinel and `0x7fff_ffff` the maximum (→ [`README.md`](README.md) § Capability Handle). The method is reached only through its public surface; [transport-boundary](../spec/behavior/transport-boundary.md) holds what that excludes.

Only the `block_given` flag travels; the block body stays inside the Guest Binary and runs through the Yield Round-Trip. The block flag's behavior lives in [transport-yield](../spec/behavior/transport-yield.md), the argument frame's in [payload-encoding](../spec/behavior/payload-encoding.md).

---

## Reply

The answer to one dispatch Call.

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | `0` success · `1` fault |
| `body` | remainder | `tag=0`: the codec-encoded value, a primitive or a Capability Handle · `tag=1`: a Fault |

Success-versus-fault is decided at this layer, so a guest learns whether the Service returned or raised from one byte, whatever the payload schema. That is why the fault rides its own arm rather than a reserved payload value. There is no partial success or streaming answer.

### Fault

The host refusing or failing a Call. Every byte is kobako's, so it rides the envelope and a guest reads it with no codec.

| Field | Type | Meaning |
|-------|------|---------|
| `kind` | `u8` | `0` runtime · `1` argument · `2` undefined · `3` internal · `4` block |
| `message` | `bytes` | human-readable description as UTF-8 |

The category is a tag, so both sides spell it alike without agreeing on text. The reserved kinds are stable across releases, and their semantics never change in place.

| Kind | Failure it represents |
|---|---|
| `runtime` | a Ruby exception raised inside the Service method |
| `argument` | argument binding failed: unknown keyword or arity |
| `undefined` | no reachable target or method |
| `internal` | the exchange itself failed, with no Service outcome |
| `block` | the caller's own block failed and the Service did not rescue it |

`undefined` covers every unresolved cause, so an opaque target discloses nothing about which methods it defines. `internal` stays apart from `runtime` because retrying against a working host could succeed. An unknown kind or trailing field degrades rather than fails, which keeps an addition survivable by an older peer. An unknown kind degrades to `undefined`, which never claims the Service ran; [envelope](../spec/behavior/envelope.md) holds that degradation.

A Fault carries no backtrace, path, or object graph. A host backtrace names paths and code the guest cannot see, which the boundary cannot bound. That is why a Fault and an Error Record stay separate types: a Panic and a Yield Reply error carry a backtrace because they flow untrusted→trusted.

---

## Yield Call and Yield Reply

A Service yielding to a block from a Call with `block_given=1` makes the Host Gem re-enter the Guest Binary. This is the reverse-direction pair: the host issues the Call and the guest answers, nested inside the dispatch frame the host is still answering.

| Aspect | Contract |
|--------|----------|
| Initiator | the Yielder passed to the Service method |
| Responder | the Guest Binary, inside the current dispatch frame |
| Synchronicity | nests strictly within the producing dispatch frame |
| Scope | valid only while that frame lives |
| Nesting | LIFO frames, one Yielder each; the Wasm stack bounds depth |

To the Service method, `yield` is an ordinary synchronous call. [transport-yield](../spec/behavior/transport-yield.md) holds how each exit unwinds.

| Message | Field | Type | Meaning |
|---------|-------|------|---------|
| Yield Call | payload | whole message | the yield arguments, codec-encoded |
| Yield Reply | `tag` | `u8` | `0x01` ok · `0x02` break · `0x04` error |
| Yield Reply | `body` | remainder | ok or break value, codec-encoded · or an Error Record |

A Yield Call carries only the yield arguments, because its enclosing dispatch frame already supplies the target and block. The ABI's `req_len` frames it, so no length prefix repeats (→ [`abi.md`](abi.md) § ABI Signatures).

| Tag | Variant | Host yield site |
|-----|---------|-----------------|
| `0x01` | **ok** | returns the block value to the Service |
| `0x02` | **break** | ends the Service with the `break` value |
| `0x03` | RESERVED | wire violation |
| `0x04` | **error** | re-raises the named class |

An ok payload follows the Reply type mapping in [`payload-msgpack.md`](payload-msgpack.md). Its Handles are restored because host code consumes it. A break value returns to the guest instead, so its Handles ride back unchanged. An answer outside the live tags, or one the host cannot frame, fails the Service's yield as a trap (→ [`../spec/behavior/transport-yield.md`](../spec/behavior/transport-yield.md)).

### Error Record

The guest's report that something it ran raised. Block and invocation failures share it, and the host re-raises from these fields with no codec.

| Field | Type | Meaning |
|-------|------|---------|
| `name` | `bytes` | the error's name as UTF-8 (`"RuntimeError"`, `"LocalJumpError"`) |
| `message` | `bytes` | human-readable description as UTF-8 |
| `backtrace` | `list<bytes>` | mruby backtrace, one UTF-8 line each; may be empty |

It differs from a Fault, which travels host to guest with a category instead of an error's own name.

---

## Outcome

The per-invocation result. The guest writes it to OUTCOME_BUFFER at the end of `__kobako_eval` or `__kobako_run`, and the host reads it through `__kobako_take_outcome`.

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | `0x01` ok · `0x02` Panic |
| `body` | remainder | `tag=0x01`: the codec-encoded value · `tag=0x02`: a Panic |

| Variant | Carries | Host reads it as |
|---------|---------|------------------|
| **ok** | the last expression, or the entrypoint's `#call` return | `Execution#value` |
| **panic** | `origin`, `name`, `message`, `backtrace`, `available` | `Kobako::ServiceError` or `Kobako::SandboxError` |

The ok body is the value alone, since the `tag` already discriminates. An empty buffer or unknown tag is refused, and bytes the host cannot frame leave nothing to attribute, so they take the trap path (→ [`../spec/behavior/outcome.md`](../spec/behavior/outcome.md)). Guest stdout and stderr never join attribution; they surface as `Execution#stdout` and `Execution#stderr`.

### Panic

The Error Record plus the fields attribution and correction need.

| Field | Type | Meaning |
|-------|------|---------|
| `origin` | `bytes` | `"sandbox"` or `"service"` as UTF-8 |
| `name` | `bytes` | the error's name as UTF-8 |
| `message` | `bytes` | exception message as UTF-8 |
| `backtrace` | `list<bytes>` | mruby backtrace, one UTF-8 line each |
| `available` | `list<bytes>` | names the failed `#run` entrypoint could have used; may be empty |

Panic carries no codec-encoded field. A host attributes a failure from `origin` and reports its correction from `available` without decoding a payload byte. Only `origin="service"` attributes to the Service, so an unreserved origin cannot widen what a Service may claim (→ [`../spec/behavior/outcome.md`](../spec/behavior/outcome.md)).

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

The stdin frames every invocation entry point reads (→ [`abi.md`](abi.md) § Invocation channels). Each frame's `[u32 be][bytes]` prefix is the transport's, not part of these layouts.

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
