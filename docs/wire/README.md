# Wire

This directory holds the wire between the Host Gem and the Guest Binary: the shape and bytes of every message during an invocation, and the ABI that carries them. Both sides implement it independently, and a kobako release ships exactly one version of it.

| Document | Holds |
|----------|-------|
| this one | how the two layers relate, the transport roles, the Capability Handle, and how each layer is held consistent |
| [`abi.md`](abi.md) | the ABI surface and the version that pins it |
| [`envelope.md`](envelope.md) | the core envelope: each message's fields, their meaning, and their bytes |
| [`payload-msgpack.md`](payload-msgpack.md) | the default payload codec |

ABI function names, packed return conventions, and the byte values in either layer document change only with an ABI version increment (→ [`abi.md`](abi.md) § ABI Version). A field or closed-set value the contract adds is not such a change: a reader degrades what it predates (→ [`envelope.md`](envelope.md) § Fault).

---

## How the Two Layers Relate

The core envelope carries only what routing and attribution need, so a side reaches a decision without decoding a payload byte. Everything the resolved method consumes rides in the opaque `payload` the codec owns.

```
┌───────────── core envelope ─────────────┐
│ routing fields │ tag │ payload (opaque) │
└──────────────────────────────┬──────────┘
                               └── payload codec
```

| Layer | Carries | Read by | Byte reference | Implemented in |
|-------|---------|---------|----------------|----------------|
| Core envelope | routing, ok-versus-fault, attribution | every side, without touching the payload | [`envelope.md`](envelope.md) | `crates/kobako-transport`, both sides |
| Payload codec | what the resolved method consumes | the two endpoints' chosen codec | [`payload-msgpack.md`](payload-msgpack.md) | `lib/kobako/` ↔ `crates/kobako-codec` |

| Property | Consequence |
|----------|-------------|
| Replaceable codec | a shared schema swaps out MessagePack entirely |
| Separable decodes | a routing-only endpoint needs no codec |

MessagePack is the default codec, not the only one. A guest shell names its codec at `MrbGuest::Codec`; a Rust host builds the SDK without its `msgpack` feature. `rake gate:payload:optional` checks that no codec appears in either side's codec-free graph. A codec substitution changes neither the ABI surface nor the envelope layout. What a replacement codec owes is in [`customization.md`](../guides/customization.md) § Codec Obligations.

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

The Host App has no API to create or inspect Handles. [transport-dispatch](../spec/behavior/transport-dispatch.md) and [transport-boundary](../spec/behavior/transport-boundary.md) hold how each property behaves.

### Handle Lifetime

The last property comes from the table's lifetime, not from listing allocation sites.

1. Each invocation mints a fresh table.
2. Its IDs ascend from 1.
3. The whole table is discarded when the invocation ends, taking un-delivered IDs with it.

This lets an opaque payload carry Handle IDs safely. Under a foreign codec a guest can write any integer as an ID. It still reaches only an object it already holds, or nothing. A Handle as a Call's target is in [`envelope.md`](envelope.md) § Call; as a value, in [`payload-msgpack.md`](payload-msgpack.md) § Ext Types.

---

## Wire-Symmetric Peers

The payload codec has two independent implementations; the core envelope has one, shared by both sides. The payload peers cannot share source: the gem's codec must load without a built native extension, and a `wasm32-wasip1` guest cannot embed Ruby.

| Layer | Host | Guest | Cross-check |
|-------|------|-------|-------------|
| Core envelope | `crates/kobako-transport` | `crates/kobako-transport` | Golden vectors against [`envelope.md`](envelope.md) |
| Payload codec | `lib/kobako/` | `crates/kobako-codec` | Cross-language (Ruby ↔ Rust) |

### Layer Split

The two layers differ because ambiguity does. Two languages disagree about types, so the payload earns a second implementation. The envelope is the fixed tier every assembly composes against, so one definition is the guarantee.

| Layer | What its peers must agree on | Guarantee |
|-------|------------------------------|-----------|
| Payload codec | 11 wire types, 2 ext codes, str/bin, Symbol `kwargs` | a second implementation |
| Core envelope | three routing fields and a byte string | one definition, held to its layout document |

Every envelope this directory specifies exists as a wire-codable type in `kobako-transport`.

### Peer Checks

Each check reaches what the one before it cannot. The guest's value walk sits below both peers and names no payload type, so it can only lose a value's fidelity, never a type's shape.

| Check | Holds | Reaches |
|-------|-------|---------|
| Round-trip fuzz | the payload peers, byte for byte | every shape the harness generates |
| Identity law | the guest's value walk | a value through the real Guest Binary |
| Payload oracle | the set of payload types each peer carries | a host type the oracle lacks, or the reverse |
| `sumi verify` | each peer's encode and decode for a registered type | a peer that drops or reshapes its half |

A payload type is registered on both peers in [`spec/contract/wire.md`](../spec/contract/wire.md). That both peers carry the same set is a promise of [payload encoding](../spec/behavior/payload-encoding.md). The oracle lists the host's types by reflection and the guest's from the one table it dispatches by. A guest type left out of that table stays outside every check.

Field names inside a type stay outside every check. A peer spells a field the way its language makes idiomatic, since the wire position is what the contract fixes. Success and failure are likewise each language's idiom: a value on the guest (`Outcome`), return-or-raise on the host.

---

## Consistency Guarantee

Each layer is held to a second source not derived from its implementation, so no layer's output defines its own correctness.

| Layer | Second source | Mechanism |
|-------|---------------|-----------|
| Core envelope | [`envelope.md`](envelope.md) | golden vectors per frame, `kind` and `tag` byte |
| Payload codec | an implementation in another language | round-trip fuzz, Ruby ↔ Rust |

The split follows where ambiguity lives. The type mapping is where two languages' conventions disagree, so that layer earns a second implementation. The envelope's peers are both Rust and share a few routing fields, so its layout document is its second source. A failure at either layer is a wire regression that blocks release.

### Golden Vectors

A golden vector spells each discriminant as the literal byte the layout document fixes. A vector written from the encoder's constant would only restate the implementation.

```
envelope.md --hand-derived--> golden vector <--compared-- kobako-transport
```

### Round-Trip Fuzz

The payload codec's fuzz harness holds both peers to each other over bytes, however they are connected:

1. Run Host → Guest → Host and Guest → Host → Guest; each ends in deep equality with the original.
2. Cover all 11 wire types, both ext types, and nested compositions such as an array of Handles.
3. Round-trip a structure at the nesting bound, and fail cleanly — never trap — one past it, a reference cycle included.
4. Take the seed from an environment variable, and print it in every failure so the seed alone reproduces the run.
5. Fail the run when any wire type or ext type went unobserved, independent of byte equality.

Iteration count is the implementer's choice. The type mapping lives in [`payload-msgpack.md`](payload-msgpack.md) § Type Mapping.
