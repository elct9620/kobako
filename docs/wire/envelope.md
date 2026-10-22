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

A decode consumes the message exactly or fails, so a framing desync fails loudly. The exceptions are a Fault and an Unsupported, which skip trailing fields they predate (→ § Fault). An envelope nested as another's last field takes the remainder as its own extent.

---

## Message Layer

Every downcall and upcall message is a tag and a body. The framing rule applies once more: the body is the remainder.

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | the operation, or the arm of its answer |
| `body` | remainder | that tag's layout, below |

A request's body is a Call, a Run, a payload, or nothing. An answer's tag is its arm. A Reply, a Yield Reply, a Compile Reply, and an Outcome are each this layer, with the arms its operation allows.

### Tag Registry

All tags share one number space, so a byte means one thing in every message. The registry grows append-only; the framing does not.

| Tag | Name | Sent on | Body |
|---|---|---|---|
| `0x00` | — | — | invalid |
| `0x01` | eval | downcall | user source, UTF-8 |
| `0x02` | run | downcall | a Run |
| `0x03` | yield | downcall | a Yield Call |
| `0x04` | call | upcall | a Call |
| `0x05` | bindings | Preamble | the bound paths |
| `0x06` | snippets | Preamble | the preloaded Snippets |
| `0x07` | flag | Preamble | a Flag |
| `0x08` | compile | downcall | none |
| `0x10` | ok | an answer to any | that operation's value |
| `0x11` | fault | an answer to call | a Fault |
| `0x12` | break | an answer to yield | the break value |
| `0x13` | error | an answer to yield, compile | an Error Record |
| `0x14` | panic | an answer to eval, run | a Panic |
| `0x15` | unsupported | an answer to any | an Unsupported |

`0x09`–`0x0F` and `0x16`–`0x7F` are kobako's, unassigned. `0x80`–`0xEF` are left for either side later, and `0xF0`–`0xFF` for third parties. A field's own `kind`, such as a Call's target form, picks a field rather than a message, so it keeps its own numbers.

### Unknown Tags

A request tag the registry has not assigned reads as unknown, with its body intact. A receiver of its own may serve one, and kobako's endpoints answer it with `unsupported`. An assigned tag already means one thing, so it is never unknown, and a writer cannot make an unknown request under one. A misplaced assigned tag is a wire violation (→ § Framing Rule).

| Received | Reader's answer |
|---|---|
| a request tag it serves | the operation's own answer |
| a request tag it does not serve | `unsupported` |
| the other direction's request tag, or an answer's arm | refused |
| request tag `0x00` | refused |
| an answer arm its operation does not allow | refused |

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

| Tag | Arm | Body |
|-----|-----|------|
| `0x10` | ok | the codec-encoded value |
| `0x11` | fault | a Fault |
| `0x15` | unsupported | an Unsupported |

There is no partial success or streaming answer.

### Unsupported

An Unsupported is a receiver saying it does not serve the operation it was sent. It answers rather than fails, so the caller decides whether to fall back or give up.

| Field | Type | Meaning |
|-------|------|---------|
| `kind` | `u8` | `0`, the only kind defined |
| `message` | `bytes` | a UTF-8 description |

It carries no tag of its own, since the caller knows what it sent. It degrades the way a Fault does: a reader takes a kind it predates as `0` and skips trailing fields it predates.

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

A Yield Call names the open Call whose block it runs, since a Service may run an outer Call's block from inside a later one. The target and method stay with that Call.

| Field | Type | Meaning |
|-------|------|---------|
| `frame` | `u32` | the open Call, counted from `0` at the outermost |
| `args` | remainder | the arguments, codec-encoded |

The Yield Reply answers by arm:

| Tag | Variant | Body | Host yield site |
|-----|---------|------|-----------------|
| `0x10` | ok | the block value | returns it to the Service |
| `0x12` | break | the `break` value | ends the Service with it |
| `0x13` | error | an Error Record | re-raises the named class |
| `0x15` | unsupported | an Unsupported | raises that the guest serves no block |

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

## Compile Reply

The Compile Reply answers a `compile` downcall. The guest compiles the Preamble's source Snippets without running them, so a host can send bytecode in their place.

| Tag | Arm | Body |
|-----|-----|------|
| `0x10` | ok | `list<bytes>`: one bytecode per source Snippet, in Preamble order |
| `0x13` | error | an Error Record |
| `0x15` | unsupported | an Unsupported |

A bytecode entry is what a bytecode Snippet carries, so the host replays it unchanged. The host keeps sending the source on every other arm, and on an ok whose count is not the source Snippets' count.

---

## Outcome

The Outcome is the per-invocation result, the answer to an `eval` or `run` downcall.

| Tag | Arm | Body |
|-----|-----|------|
| `0x10` | ok | the codec-encoded value |
| `0x14` | panic | a Panic |
| `0x15` | unsupported | an Unsupported |

The ok value becomes `Execution#value`. An empty answer or unknown tag is refused, and bytes the host cannot frame take the trap path ([outcome](../spec/behavior/outcome.md)). Guest stdout and stderr never join attribution.

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

Run is the host→guest entrypoint dispatch, the body of a `run` downcall.

| Field | Type | Meaning |
|-------|------|---------|
| `entrypoint` | `bytes` | a top-level constant name, `/\A[A-Z]\w{0,65533}\z/` |
| `payload` | remainder | the entrypoint's arguments, codec-encoded |

It carries no `method`, since the entrypoint is invoked through `#call`, and no `block_given`, since `#run` supplies no block.

---

## Preamble

The Preamble is what each invocation reads from stdin before its entry runs (→ [`abi.md`](abi.md) § Invocation inputs). It is a counted list of records, so a new kind of setup joins as a registry entry rather than a new position.

| Field | Type | Meaning |
|-------|------|---------|
| `count` | `u32` | records that follow |
| per record `critical` | `u8` | `0` advisory · `1` critical |
| per record `kind` | `u8` | a tag from the registry's Preamble rows |
| per record `body` | `bytes` | that kind's layout, below |

A reader that does not know a record's kind skips it when advisory and refuses the invocation when critical. A kind the registry assigns to a message is refused either way. Bindings and snippets each appear at most once, and an absent one reads as empty, so an empty Preamble is `count = 0`.

### Preamble Records

Each body's shape is fixed by its kind.

| Kind | Field | Type | Meaning |
|------|-------|------|---------|
| bindings | `paths` | `list<bytes>` | each bound constant's path |
| snippets | `count` | `u32` | entries that follow |
| snippets, per entry | `kind` | `u8` | `0` mruby source · `1` RITE bytecode |
| snippets, per entry | `name` | `bytes` | only when `kind=0` |
| snippets, per entry | `body` | `bytes` | the source or the bytecode |
| flag | `name` | `bytes` | the switch, UTF-8 |
| flag | `value` | `bytes` | UTF-8; empty for a plain switch |

Snippets replay in insertion order, and a source snippet's backtraces show `(snippet:<name>)`. A bytecode entry carries no name, because its filename comes from its own debug info ([sandbox](../spec/behavior/sandbox.md)). A Guest Binary that does not recognise a flag's name treats it as it would an unknown kind, by the record's `critical`.

---

## Message Bounds

Every message, the Preamble included, is at most 16 MiB. The host enforces it in both directions, so a guest-supplied length never makes the host allocate past it.

| Direction | The host checks |
|-----------|-----------------|
| guest → host | a Call, Outcome, Yield Reply, or Compile Reply, before reading it |
| host → guest | a downcall request or a Reply, before writing it |
| host → guest stdin | the Preamble, before sending it |

The guest's Preamble reader caps a declared length at 64 MiB. That ceiling only guards an allocation from a length prefix; the contract is the 16 MiB. A `memory_limit` below 16 MiB binds first, since the guest grows memory to hold what it reads.

This layer is non-recursive, so it has no depth to budget. Each payload has its own (→ [`payload-msgpack.md`](payload-msgpack.md) § Structural Nesting Depth).
