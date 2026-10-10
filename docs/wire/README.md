# Wire

The wire is every message between the Host Gem and the Guest Binary during an invocation. Both sides implement it independently, and a kobako release ships exactly one version of it.

| Document | Holds |
|----------|-------|
| this one | the two layers, transport roles, Handles, consistency |
| [`abi.md`](abi.md) | the ABI surface and its version |
| [`envelope.md`](envelope.md) | each message's fields, meaning, and bytes |
| [`payload-msgpack.md`](payload-msgpack.md) | the default payload codec |

ABI names, return conventions, and the byte values in either layer change only with an ABI version increment (→ [`abi.md`](abi.md) § ABI Version). An added field or closed-set value is not such a change, because a reader degrades what it predates (→ [`envelope.md`](envelope.md) § Fault).

---

## Two Layers

Every message is a core envelope around an opaque payload. The envelope carries what routing and attribution need, so a side reaches those decisions without decoding a payload byte.

```
┌─────────── core envelope ───────────┐
│ envelope fields │ payload (opaque)  │
└──────────────────────────┬──────────┘
                           └── payload codec
```

| Layer | Carries | Read by | Implemented in |
|-------|---------|---------|----------------|
| Core envelope | routing, ok-versus-fault, attribution | every side | `kobako-transport`, shared |
| Payload codec | what the resolved method consumes | the two endpoints | `lib/kobako/` ↔ `kobako-codec` |

The codec is therefore replaceable, and a routing-only endpoint needs none. MessagePack is the default, not the only one; a codec swap changes neither the ABI nor the envelope. `rake gate:payload:optional` checks that each codec-free tier builds without one. What a replacement codec owes is in [`customization.md`](../guides/customization.md) § Codec Obligations.

---

## Transport Role

Every exchange is one **Call** answered by one **Reply**. The roles name which side speaks first, not which side is host or guest.

| Role | Side | Scope |
|------|------|-------|
| Conversation opener | Guest Binary (`Kobako::Proxy`) | every conversation |
| Responder | Host Gem | inside the same import call frame |
| Reverse-direction Call | Host Gem | only to re-enter a block |
| Medium | Wasm linear memory | an implementation note |

Every round-trip is synchronous. To guest code, a Service call is an ordinary call that completes before the next line.

---

## Capability Handle

A Capability Handle is an opaque token for a stateful Ruby object the wire cannot represent, such as a session or a `StringIO`.

| Property | Contract |
|----------|----------|
| Opaque | the guest can only pass it back or call methods on it |
| Host-allocated | minted for a stateful Service answer or `#run` argument |
| Scoped to one invocation | each invocation mints its own table |
| Not constructible | no guest or Host App API turns an integer into one |
| ID range | `1` to `0x7fff_ffff`; `0` is the invalid sentinel |
| Exhaustion | past the cap raises `Kobako::HandleExhaustedError` |

The Host App has no API to create or inspect Handles. [transport-dispatch](../spec/behavior/transport-dispatch.md) and [transport-boundary](../spec/behavior/transport-boundary.md) hold how each property behaves.

### Handle Lifetime

Every minted ID reaches the guest in the message that minted it. That holds because of the table's lifetime:

1. Each invocation mints a fresh table.
2. Its IDs ascend from 1.
3. The table is discarded when the invocation ends, with any undelivered IDs.

An opaque payload can therefore carry Handle IDs safely. Under a foreign codec a guest can write any integer as an ID, and still reaches only an object it already holds, or nothing.

---

## Consistency Guarantee

Each layer is held to a second source not derived from its implementation, so no implementation defines its own correctness. A failure at either layer is a wire regression that blocks release.

| Layer | Implementations | Second source |
|-------|-----------------|---------------|
| Core envelope | one, shared by both sides | [`envelope.md`](envelope.md) |
| Payload codec | two, Ruby and Rust | each other |

The split follows where ambiguity lives. Two languages disagree about types, so the payload earns a second implementation. The envelope is a few routing fields every assembly composes against, so one definition held to its layout document is the guarantee.

### Envelope Vectors

Golden vectors spell each discriminant as the literal byte [`envelope.md`](envelope.md) fixes. A vector copied from the encoder's constant would only restate the implementation.

```
envelope.md --hand-derived--> golden vector <--compared-- kobako-transport
```

### Payload Peers

The two codecs cannot share source: the gem's must load without a native extension, and a `wasm32-wasip1` guest cannot embed Ruby. Each guarantee below is held where it can be observed.

| Guarantee | Held by |
|-----------|---------|
| both peers write the same bytes for a value | a cross-language round-trip |
| both peers carry the same set of payload types | the payload oracle |
| each peer encodes and decodes a registered type | `sumi verify` |
| a value through the real Guest Binary survives | the guest value fuzz |
| past the nesting bound or on a cycle, a clean failure, never a trap | unit and e2e tests |

A payload type is registered on both peers in [`spec/contract/wire.md`](../spec/contract/wire.md), and [payload encoding](../spec/behavior/payload-encoding.md) promises both carry the same set. A guest type left out of the table the guest dispatches by stays outside every check.

### Peer Idiom

The contract fixes wire positions, not spellings. These stay each language's own.

| Left to each peer | Example |
|-------------------|---------|
| field names inside a type | Ruby keyword versus Rust field |
| success and failure | a value on the guest, return-or-raise on the host |
