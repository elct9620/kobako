# Core Envelope

The core envelope is the outer frame of every host↔guest message. This document pins each field's meaning and bytes. The layout is fixed, not MessagePack, and hands everything but routing and attribution through as an opaque `payload`.

| Document | Holds |
|----------|-------|
| [`README.md`](README.md) | the two layers, transport roles, Handles |
| [`abi.md`](abi.md) | the functions that carry each message |
| [`payload-msgpack.md`](payload-msgpack.md) | the default payload codec |
| [`spec/behavior/envelope.md`](../spec/behavior/envelope.md) | the envelope's behavior |

`kobako-transport` implements this layout once, for both sides, and its golden vectors derive from this document (→ [`README.md`](README.md) § Consistency Guarantee).

---

## Layer Properties

Every layout below has both properties, so a side routes a Call and attributes a failure from these fields alone.

| Property | Consequence |
|----------|-------------|
| Decodable without the payload codec | the codec is replaceable |
| Non-recursive | nothing to bound, no stack to overflow on untrusted input |

Every field is a scalar, a byte string, or a flat list.

---

## Primitives

Every core envelope is built from four primitives. All integers are unsigned big-endian.

| Primitive | Layout |
|-----------|--------|
| `u8` | one byte |
| `u32` | four bytes, big-endian |
| `bytes` | `u32` length, then that many bytes |
| `list<bytes>` | `u32` count, then that many `bytes` values |

A `bytes` field of length `0` is a present, empty value. A field that can mean "absent" says so in its row.

### Framing Rule

Every field except the last is self-delimiting; the last consumes the rest of the message. Its length is already known from the frame prefix or the ABI's `len`, so repeating it would give one fact two sources.

```
[ field ][ field ] ... [ last field ............ ]
 self-delimiting        remainder of the message
```

A decode consumes the message exactly or fails, so a framing desync fails loudly. The one exception is a Fault, which skips trailing fields it predates (→ § Fault). An envelope nested as another's last field takes the remainder as its own extent.

---

## Call

A Call is the guest→host dispatch. Its envelope fields route it; its payload feeds the method.

| Field | Type | Meaning |
|-------|------|---------|
| `kind` | `u8` | `0` constant path · `1` Capability Handle |
| `target` | `bytes` or `u32` | a UTF-8 path, or a Handle ID |
| `method` | `bytes` | one UTF-8 method name |
| `block_given` | `u8` | `0` or `1` |
| `payload` | remainder | the arguments, codec-encoded |

`kind` tells the two targets apart without reading anything else. A path uses Ruby constant syntax (`"MyService::KV"`), matching the guest's own constant access. A Handle target is an envelope field, not a payload value, so a codec without a Handle representation still reaches one.

The method is one name, reached only through its public surface ([transport-boundary](../spec/behavior/transport-boundary.md)). Only the block flag travels; the block stays in the guest and runs through a Yield Call ([transport-yield](../spec/behavior/transport-yield.md)).

---

## Reply

A Reply answers one Call. Success versus fault is decided by one byte, whatever the payload schema, so a fault rides its own arm rather than a reserved payload value.

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | `0` success · `1` fault |
| `body` | remainder | the codec-encoded value · or a Fault |

There is no partial success or streaming answer.

### Fault

A Fault is the host refusing or failing a Call. Every byte is kobako's, so it rides the envelope and a guest reads it with no codec.

| Field | Type | Meaning |
|-------|------|---------|
| `kind` | `u8` | the failure category, below |
| `message` | `bytes` | a UTF-8 description |

A Fault carries no backtrace, path, or object graph: a host backtrace names code the guest cannot see. A Panic and a Yield Reply error carry one because they flow untrusted to trusted.

A reader degrades what it predates instead of failing. An unknown kind reads as `undefined`, which never claims the Service ran, and unknown trailing fields are skipped ([envelope](../spec/behavior/envelope.md)).

### Fault Kinds

The reserved kinds keep their meaning across releases.

| `kind` | Name | Failure |
|---|---|---|
| `0` | `runtime` | the Service method raised |
| `1` | `argument` | argument binding failed |
| `2` | `undefined` | no reachable target or method |
| `3` | `internal` | the exchange failed, no Service outcome |
| `4` | `block` | the caller's block failed, unrescued |

`undefined` covers every unresolved cause, so an opaque target discloses nothing about its methods. `internal` stays apart from `runtime` because a retry against a working host could succeed.

---

## Yield Messages

A Service yielding to a block from a Call with `block_given=1` makes the host re-enter the guest. The host issues a Yield Call; the guest answers with a Yield Reply, inside the dispatch frame the host is still answering.

| Aspect | Contract |
|--------|----------|
| Initiator | the Yielder passed to the Service method |
| Synchronicity | nests strictly within its dispatch frame |
| Scope | valid only while that frame lives |
| Nesting | LIFO frames, one Yielder each |

To the Service method, `yield` is an ordinary synchronous call ([transport-yield](../spec/behavior/transport-yield.md)).

### Yield Layouts

A Yield Call carries only the arguments, since its dispatch frame already supplies target and block. The ABI's `req_len` frames it, so no prefix repeats.

| Message | Field | Type | Meaning |
|---------|-------|------|---------|
| Yield Call | payload | whole message | the arguments, codec-encoded |
| Yield Reply | `tag` | `u8` | the variant, below |
| Yield Reply | `body` | remainder | a value, or an Error Record |

| Tag | Variant | Body | Host yield site |
|-----|---------|------|-----------------|
| `0x01` | ok | the block value | returns it to the Service |
| `0x02` | break | the `break` value | ends the Service with it |
| `0x03` | reserved | — | wire violation |
| `0x04` | error | an Error Record | re-raises the named class |

An ok value's Handles are restored, because host code consumes it. A break value returns to the guest, so its Handles ride back unchanged. Any other tag, or a reply the host cannot frame, fails the yield as a trap.

### Error Record

An Error Record is the guest's report that something it ran raised. Block and invocation failures share it, and the host re-raises from it with no codec.

| Field | Type | Meaning |
|-------|------|---------|
| `name` | `bytes` | the error's class name (`"LocalJumpError"`) |
| `message` | `bytes` | a UTF-8 description |
| `backtrace` | `list<bytes>` | mruby backtrace lines; may be empty |

It differs from a Fault, which travels host to guest with a category instead of an error's own name.

---

## Outcome

The Outcome is the per-invocation result. The guest writes it at the end of `__kobako_eval` or `__kobako_run`; the host takes it through `__kobako_take_outcome`.

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | `0x01` ok · `0x02` Panic |
| `body` | remainder | the codec-encoded value · or a Panic |

The ok value becomes `Execution#value`. An empty buffer or unknown tag is refused, and bytes the host cannot frame take the trap path ([outcome](../spec/behavior/outcome.md)). Guest stdout and stderr never join attribution.

### Panic

A Panic is an Error Record plus what attribution and correction need. It has no codec-encoded field, so the host attributes a failure without decoding a payload byte.

| Field | Type | Meaning |
|-------|------|---------|
| `origin` | `bytes` | `"sandbox"` or `"service"` |
| `name` | `bytes` | the error's class name |
| `message` | `bytes` | the exception message |
| `backtrace` | `list<bytes>` | mruby backtrace lines |
| `available` | `list<bytes>` | entrypoints a failed `#run` could use; may be empty |

Only `"service"` attributes to the Service (`Kobako::ServiceError`); anything else is the Sandbox's (`Kobako::SandboxError`). An unreserved origin therefore cannot widen what a Service may claim.

---

## Run

Run is the host→guest entrypoint dispatch, delivered on the command buffer to `__kobako_run`.

| Field | Type | Meaning |
|-------|------|---------|
| `entrypoint` | `bytes` | a top-level constant name, `/\A[A-Z]\w{0,65533}\z/` |
| `payload` | remainder | the entrypoint's arguments, codec-encoded |

It carries no `method`, since the entrypoint is invoked through `#call`, and no `block_given`, since `#run` supplies no block.

---

## Invocation Frames

These are the stdin frames each entry point reads (→ [`abi.md`](abi.md) § Invocation channels). Frame 2 is raw `#eval` source with no envelope; the other two have these layouts.

| Frame | Field | Type | Meaning |
|-------|-------|------|---------|
| 1 preamble | `paths` | `list<bytes>` | each bound constant's path; may be empty |
| 3 snippets | `count` | `u32` | entries that follow |
| 3, per entry | `kind` | `u8` | `0` mruby source · `1` RITE bytecode |
| 3, per entry | `name` | `bytes` | only when `kind=0` |
| 3, per entry | `body` | `bytes` | the source or the bytecode |

Snippets replay in insertion order, and a source snippet's backtraces show `(snippet:<name>)`. A bytecode entry carries no name, because its filename comes from its own debug info ([sandbox](../spec/behavior/sandbox.md)).

---

## Size and Depth Bounds

Every message, invocation frames included, is at most 16 MiB. The host enforces it in both directions, so a guest-supplied length never makes the host allocate past it.

| Direction | The host checks |
|-----------|-----------------|
| guest → host | a Call, Outcome, or Yield Reply, before reading it |
| host → guest | a Reply, Yield Call, or Run, before writing it |
| host → guest frames | each invocation frame, before sending it |

The guest's frame reader caps a declared length at 64 MiB. That ceiling only guards an allocation from a length prefix; the contract is the 16 MiB. A `memory_limit` below 16 MiB binds first, since the guest grows memory to hold what it reads.

This layer is non-recursive, so it has no depth to budget. Each payload has its own (→ [`payload-msgpack.md`](payload-msgpack.md) § Structural Nesting Depth).
