# Wire Contract

This document gives the logical shape of every message between the Host Gem and the Guest Binary during an invocation. Both sides implement it independently, and a kobako release ships exactly one version of it.

| Part | Carries | Read by |
|------|---------|---------|
| Envelope | routing and outcome attribution | every side, without touching the payload |
| Payload | what the resolved method consumes | the two endpoints' chosen codec |

A side reads an envelope without interpreting a payload byte, so the endpoints choose the payload encoding. Byte-level encoding of both parts is anchored in [`wire-codec.md`](wire-codec.md).

---

## Transport Role

Every exchange is one **Call** answered by one **Reply**. The roles name which side speaks first in a round-trip, not which side is host or guest.

| Role | Side | Scope |
|------|------|-------|
| Conversation opener | Guest Binary (the `Kobako::Proxy` mix-in) | every conversation |
| Responder | Host Gem | inside the same Wasm import call frame |
| Reverse-direction Call | Host Gem | only to re-enter a block, nested in a dispatch |
| Medium | Wasm linear memory | an implementation note, not contract |

Every round-trip is synchronous. To the guest script, a Service method call is an ordinary call that completes before the next line, with no callbacks or promises.

---

## Call Shape

A Call separates the envelope fields that **route** it from the payload fields that **feed** the method.

| Part | Field | Type | Meaning |
|------|-------|------|---------|
| Envelope | `target` | constant path (`"MyService::KV"`, `"File"`) or Capability Handle | the receiving object |
| Envelope | `method` | string | one method name, no multi-segment traversal |
| Envelope | `block_given` | bool | whether the call site supplied a block |
| Payload | `args` | ordered list | positional arguments, Handles allowed |
| Payload | `kwargs` | key-value map | keyword arguments, Symbol keys, Handles allowed |

The two `target` forms are distinguishable without reading `method` or the payload. The string form uses Ruby constant-path syntax, so the wire value matches the guest's own constant access. The method is reached only through its public surface; [transport-boundary](spec/behavior/transport-boundary.md) holds what that excludes.

Only the `block_given` flag travels; the block body stays inside the Guest Binary and runs through the Yield Round-Trip. A **Yield Call** carries only the yield arguments, because its enclosing dispatch frame already supplies the target and block. The block flag's behavior lives in [transport-yield](spec/behavior/transport-yield.md), the argument frame's in [payload-encoding](spec/behavior/payload-encoding.md).

---

## Reply Shape

A Reply splits like a Call: `tag` is the envelope, and the payload belongs to exactly one variant.

| Variant | Envelope | Payload |
|---------|----------|---------|
| **Success** | `tag=0` | `value`, a primitive or a Capability Handle |
| **Fault** | `tag=1` | fault body (see Fault) |

Success versus fault is an envelope decision, so a side learns it without reading the payload. There is no partial success or streaming answer. The Yield Reply adds a third outcome, `break`, under Yield Reply Envelope.

---

## Capability Handle

A **Capability Handle** is an opaque token for a stateful Ruby object the wire cannot represent, such as a session or a `StringIO`.

| Property | Contract |
|----------|----------|
| Opaque | the guest can only pass it back or call methods on it |
| Host-allocated | minted for a stateful Service answer or `#run` argument |
| Scoped to one invocation | each invocation mints its own Catalog::Handles |
| Not constructible | no guest or Host App API turns an integer into one |
| ID cap | `0x7fff_ffff` (2³¹ − 1); past it raises `Kobako::HandleExhaustedError` |
| No reachable un-delivered Handle | every minted ID reached the guest in the minting message |

The Host App has no API to create or inspect Handles. [transport-dispatch](spec/behavior/transport-dispatch.md) and [transport-boundary](spec/behavior/transport-boundary.md) hold how each property behaves.

### Handle Lifetime

The last property comes from the table's lifetime, not from listing allocation sites.

1. Each invocation mints a fresh table.
2. Its IDs ascend from 1.
3. The whole table is discarded when the invocation ends, taking un-delivered IDs with it.

This lets an opaque payload carry Handle IDs safely. Under a foreign codec a guest can write any integer as an ID. It still reaches only an object it already holds, or nothing. The ext type and byte layout are in [`wire-codec.md`](wire-codec.md).

---

## Fault

A Fault is the host refusing or failing a Call. It rides the core envelope, not the payload, so a guest reads a refusal with no payload codec.

| Field | Type | Meaning |
|-------|------|---------|
| `type` | enumeration | the failure category |
| `message` | string | human-readable description |

The category travels as a tag, so both sides spell it alike without agreeing on text. A Fault travels host→guest and carries no backtrace, path, or object graph, since that content would cross the trust boundary unbounded. A Panic and a Yield Reply error carry a backtrace because they flow untrusted→trusted.

### Fault Types

The reserved `type` values are stable across releases, and their semantics never change in place.

| `type` value | Failure it represents |
|---|---|
| `"runtime"` | a Ruby exception raised inside the Service method |
| `"argument"` | argument binding failed: unknown keyword or arity |
| `"undefined"` | no reachable target or method |
| `"internal"` | the exchange itself failed, with no Service outcome |
| `"block"` | the caller's own block failed and the Service did not rescue it |

`"undefined"` covers every unresolved cause, so an opaque target discloses nothing about which methods it defines. `"internal"` stays apart from `"runtime"` because retrying against a working host could succeed. The set is open: a receiver degrades a type or field it predates instead of failing. [envelope](spec/behavior/envelope.md) holds that degradation.

---

## Outcome Envelope

The outcome envelope carries the final result of a whole invocation. The guest writes it at the end of `__kobako_eval` or `__kobako_run`, and the host takes it via `__kobako_take_outcome`.

| Variant | Carries | Host reads it as |
|---------|---------|------------------|
| **ok** | the last expression, or the entrypoint's `#call` return | `Execution#value` |
| **panic** | `origin`, `name`, `message`, `backtrace`, `available` | `Kobako::ServiceError` or `Kobako::SandboxError` |

Only `origin="service"` attributes to the Service, so an unreserved origin cannot widen what a Service may claim. `available` lists the names an unresolved `#run` entrypoint could have used. Every Panic field is typed at the envelope layer, with no codec-encoded content.

Bytes the host cannot frame leave nothing to attribute, so they take the trap path. Guest stdout and stderr never join attribution; they surface as `Execution#stdout` and `Execution#stderr`. [outcome](spec/behavior/outcome.md) holds the attribution rules.

---

## Yield Round-Trip

A Service yielding to a block from a Call with `block_given=true` makes the Host Gem re-enter the Guest Binary. This is the reverse-direction pair: the host issues the Call and the guest answers.

| Aspect | Contract |
|--------|----------|
| Initiator | the Yielder passed to the Service method |
| Responder | the Guest Binary, inside the current dispatch frame |
| Synchronicity | nests strictly within the producing dispatch frame |
| Scope | valid only while that frame lives |
| Nesting | LIFO frames, one Yielder each; the Wasm stack bounds depth |

To the Service method, `yield` is an ordinary synchronous call. [transport-yield](spec/behavior/transport-yield.md) holds how each exit unwinds.

---

## Yield Reply Envelope

The Yield Reply carries one yield round-trip's outcome back to the host yield site. It appears only mid-dispatch and is a single tag byte plus an optional payload.

| Tag | Variant | Payload | Host yield site |
|-----|---------|---------|-----------------|
| `0x01` | **ok** | block value | returns it to the Service |
| `0x02` | **break** | `break` value | ends the Service with it |
| `0x03` | RESERVED | — | wire violation |
| `0x04` | **error** | Error Record | re-raises the named class |

An ok payload follows the Reply type mapping in [`payload-msgpack.md`](wire/payload-msgpack.md). Its Handles are restored because host code consumes it. A break value returns to the guest instead, so its Handles ride back unchanged. A Yield Reply the host cannot frame fails the Service's yield as a trap; [transport-yield](spec/behavior/transport-yield.md) holds every exit.

### Error Record

The error variant shares the Panic's three failure fields, so the host re-raises from either without the payload codec.

| Field | Type | Meaning |
|-------|------|---------|
| `name` | text | class name, e.g. `"LocalJumpError"`, `"TypeError"` |
| `message` | text | human-readable description |
| `backtrace` | list of text | mruby backtrace, one line per element |

---

## ABI-Versioned Contract

A single u32 ABI version pins this contract (→ [`wire-codec.md`](wire-codec.md) § ABI Version). The Host Gem accepts a Guest Binary only on equality, at Sandbox construction; [runtime](spec/behavior/runtime.md) holds the refusal.

| Rule | Consequence |
|------|-------------|
| No in-band version field | alignment checked once, not per message |
| No negotiation | each side implements one wire shape |
| Lockstep evolution | a field change bumps the version on both sides |

A sandbox is short-lived, so no connection or stored payload outlasts an ABI version. An independently built Guest Binary conforms by rebuilding against the new version. Release notes list wire-affecting changes under Breaking Changes.

---

## Wire-Symmetric Peers

The payload codec has two independent implementations; the core envelope has one, shared by both sides. The payload peers cannot share source: the gem's codec must load without a built native extension, and a `wasm32-wasip1` guest cannot embed Ruby.

| Layer | Host | Guest | Cross-check |
|-------|------|-------|-------------|
| Core envelope | `crates/kobako-transport` | `crates/kobako-transport` | Golden vectors against [`wire/envelope.md`](wire/envelope.md) |
| Payload codec | `lib/kobako/` | `crates/kobako-codec` | Cross-language (Ruby ↔ Rust) |

### Layer Split

The two layers differ because ambiguity does. Two languages disagree about types, so the payload earns a second implementation. The envelope is the fixed tier every assembly composes against, so one definition is the guarantee.

| Layer | What its peers must agree on | Guarantee |
|-------|------------------------------|-----------|
| Payload codec | 11 wire types, 2 ext codes, str/bin, Symbol `kwargs` | a second implementation |
| Core envelope | three routing fields and a byte string | one definition, held to its layout document |

Every envelope this document specifies exists as a wire-codable type in `kobako-transport`.

### Peer Checks

Each check reaches what the one before it cannot. The guest's value walk sits below both peers and names no payload type, so it can only lose a value's fidelity, never a type's shape.

| Check | Holds | Reaches |
|-------|-------|---------|
| Round-trip fuzz | the payload peers, byte for byte | every shape the harness generates |
| Identity law | the guest's value walk | a value through the real Guest Binary |
| Payload oracle | the set of payload types each peer carries | a host type the oracle lacks, or the reverse |
| `sumi verify` | each peer's encode and decode for a registered type | a peer that drops or reshapes its half |

A payload type is registered on both peers in [`spec/contract/wire.md`](spec/contract/wire.md). That both peers carry the same set is a promise of [payload encoding](spec/behavior/payload-encoding.md). The oracle lists the host's types by reflection and the guest's from the one table it dispatches by. A guest type left out of that table stays outside every check.

Field names inside a type stay outside every check. A peer spells a field the way its language makes idiomatic, since the wire position is what the contract fixes. Success and failure are likewise each language's idiom: a value on the guest (`Outcome`), return-or-raise on the host.
