# Wire Contract

This document specifies the abstract logical shape of every message exchanged between the Host Gem and the Guest Binary during a Sandbox invocation (`#eval` or `#run`). It is a **Consistency-layer contract**: both sides implement it independently, and a kobako gem release ships exactly one version of it.

Every message splits into two parts, and the split governs this whole document: the **envelope** carries what routing and outcome attribution need, and the **payload** carries what the resolved method consumes. A side reads an envelope without interpreting a payload byte, which is what lets the payload's encoding be chosen by the two endpoints rather than fixed here. Byte-level encoding of both parts is specified in [`docs/wire-codec.md`](wire-codec.md) and the two layer documents it anchors.

The governing summary of this contract lives in `SPEC.md` § Wire Contract; this document is its abstract reference.

---

## Transport Role

Every exchange is one **Call** answered by one **Reply**. Both directions use that same pair: the role names describe which side speaks first in a given round-trip, not which side is host or guest.

- **Conversation opener**: the Guest Binary (a `Kobako::Proxy` mix-in) opens every conversation. The Host Gem never pushes a Call to the guest unprompted.
- **Responder**: the Host Gem answers each Call synchronously within the same Wasm import function call frame, then returns the Reply to the guest before that frame exits.
- **Reverse-direction Call**: the Host Gem issues a Call of its own only to re-enter a Block — the Yield Call, nested inside the dispatch it is still answering (see Yield Round-Trip).
- **Synchronicity**: every round-trip is fully synchronous. From the guest mruby script's perspective, a Service method call is an ordinary synchronous function call that completes before the next line executes. There are no callbacks, promises, or yield-resume mechanisms.
- **Medium**: Wasm linear memory. The guest writes the serialized Call into linear memory and calls a Wasm import function; the host reads and writes through a memory view provided by the Wasm engine. This is an implementation note; the wire contract specifies message shape, not transport mechanics.

---

## Call Shape

A Call separates the fields that **route** it from the fields that **feed** the method it routes to. The routing fields are the envelope; the invocation arguments are the payload.

| Part | Field | Type | Meaning |
|------|-------|------|---------|
| Envelope | `target` | A bound constant's path (constant-path string `"MyService::KV"`, or a single segment `"File"`) **or** Capability Handle reference | Identifies the Ruby object that receives the call. The two forms are distinguishable on the wire without inspecting `method` or the payload. |
| Envelope | `method` | string | The single method name to invoke on the resolved target via `public_send`. One method per Call; no multi-segment traversal in a single wire call. |
| Envelope | `block_given` | bool | Whether the guest call site supplied a block. When `true`, the Host Gem materialises a Yielder and passes it to the resolved Service method as `&block` (B-23). When `false`, the Service method receives no block and `block_given?` returns `false`. The block body itself is never serialized — only this flag travels on the wire; the block remains inside the Guest Binary and is invoked through Yield Round-Trip. |
| Payload | `args` | ordered list | Positional arguments passed to the method. Elements may themselves be Capability Handle references. |
| Payload | `kwargs` | key-value map | Keyword arguments passed to the method. Keys are Symbols on the wire (→ [`docs/wire/payload-msgpack.md`](wire/payload-msgpack.md) § Ext Types → ext 0x00); the host passes them to dispatch unchanged. Values, like `args` elements, may themselves be Capability Handle references. An empty kwargs map is always present (never absent) to keep field positions stable. |

The envelope fields are what routing needs and nothing more: a side can resolve the target, name the method, and know whether a block is coming without interpreting a single payload byte. The payload is what the resolved method consumes.

The `target` string form uses Ruby constant-path syntax (`"MyService::KV"`, or a top-level `"File"`) so the wire value is identical to the guest-side constant access expression — no cognitive translation between layers.

A **Yield Call** carries only the block's yield arguments; the dispatch frame it nests in supplies the target and the block identity, so neither travels again.

---

## Reply Shape

A Reply splits the same way a Call does: `tag` is the envelope, and what follows it is the payload of exactly one of two mutually exclusive variants.

| Variant | Envelope | Payload | Meaning |
|---------|----------|---------|---------|
| **Success** | `tag=0` | `value` | The call completed successfully. `value` carries the return value (a primitive or a Capability Handle reference). |
| **Fault** | `tag=1` | fault body | The call failed. The Fault (see Fault below) describes the failure category and message. |

Success-versus-fault is an envelope decision, not a payload one: a side learns whether the call succeeded by reading `tag`, without interpreting a payload byte. A Reply always matches exactly one variant. There is no partial success or streaming answer. The Yield Reply answering a Yield Call carries a third outcome the dispatch Reply has no counterpart for — `break` — and is specified under Yield Reply Envelope.

---

## Capability Handle

A **Capability Handle** is an opaque token used on either side of the wire to reference a stateful Ruby object that is not directly wire-representable (e.g., a session, connection, `StringIO`, custom Env / Context class). The abstract contract is:

- **Opaque**: the guest receives a Handle token and cannot extract the underlying Ruby object from it; the only permitted operation is passing the token back as a `target` or `args` element in a subsequent Call, or invoking methods on it which dispatch as Transport Calls.
- **Host-allocated**: the wire layer on the host side allocates a Handle automatically in two symmetric situations — whenever a Service method returns a stateful object (host→guest return path, → [`docs/behavior/dispatch.md`](behavior/dispatch.md) § B-14), and whenever `#run` is invoked with arguments containing non-wire-representable objects (host→guest argument path, → [`docs/behavior/dispatch.md`](behavior/dispatch.md) § B-34). The Host App has no API to create or inspect Handles directly.
- **Scoped to a single invocation**: a Handle token issued during invocation N is invalid in invocation N+1. Every invocation (`#eval` or `#run`) mints its own Catalog::Handles; the scoping is uniform regardless of allocation source.
- **Not constructible by guest or Host App**: neither the guest mruby API nor the Host App API exposes a public constructor that converts a bare integer to a Handle. A raw integer presented as a Handle on the wire is rejected before it reaches the Catalog::Handles; a `Kobako::Handle` instance fabricated through any non-public path on the host side is rejected at `#run` host pre-flight. Handle allocation is exclusively internal to the Host Gem's wire layer.
- **ID cap**: the opaque ID component of a Handle is bounded by `0x7fff_ffff` (2³¹ − 1). Allocation beyond this cap raises `Kobako::HandleExhaustedError` immediately (fail-fast; no silent wraparound).
- **No reachable un-delivered Handle**: a Handle ID names an object only for as long as the invocation that minted it, and every ID that invocation minted was handed to the guest in the same message that minted it. A guest naming an arbitrary integer as a Handle therefore reaches either an object it already holds or nothing at all (B-65 → [`behavior/security.md`](behavior/security.md)). This is a property of the table's lifetime, not of an enumeration of allocation sites: the table is minted per invocation, its IDs ascend from 1, and it is discarded whole when the invocation ends — so an invocation that fails part-way through minting takes its un-delivered IDs with it.

The last property is what lets an opaque payload carry Handle IDs safely. When the payload codec is one kobako does not define, a Handle ID inside it is an ordinary integer the guest can set to any value; the boundary holds because of the table's lifetime, not because the guest was unable to write the number.

Byte-level encoding of the Capability Handle (ext type number, binary layout) is specified in [`docs/wire-codec.md`](wire-codec.md).

---

## Fault

A Fault describes a Service-layer failure. It is the whole of a Reply's fault variant and rides the core envelope, not the payload: every byte of one is kobako's, so a guest reads a refusal with no payload codec at all and a replacement codec owes it nothing. It carries two fields:

| Field | Type | Meaning |
|-------|------|---------|
| `type` | enumeration | Which failure the Fault reports, from the reserved categories below. A receiver reads one it predates as `"undefined"`. |
| `message` | string | Human-readable description of the failure. |

The category is closed, so the envelope carries it as a tag: a value outside the three is unrepresentable rather than merely unrecognised, and no endpoint has to decide what an unknown category means.

A Fault travels host→guest, and its author controls only the message. Host-side structure — a backtrace, a path, an object graph — would cross the trust boundary as content no author can bound, so a Fault carries none. The reverse direction is not symmetric: a Panic and a Yield Reply's error arm both carry a backtrace, and those flow untrusted→trusted.

The reserved `type` values are:

| `type` value | Failure it represents |
|---|---|
| `"runtime"` | A general Ruby exception raised inside a Service method during dispatch |
| `"argument"` | A Service method's argument binding failed — an unknown keyword, or an arity mismatch (E-15) |
| `"undefined"` | The `target` string path matches no registered Service, the `target` Handle ID is not live in this invocation's Catalog::Handles, or the method resolves to no reachable Service method on the target — absent, or ambient reflection/eval surface. The three cases share one type so an opaque target discloses nothing about which methods it defines |
| `"internal"` | The exchange failed with no Service outcome to report: the request was unreadable (E-10), or the host could not issue the Handle the answer needs (E-07). Separate from `"runtime"` because retrying the same call against a working host would behave differently, which is not true of a Service that raised |
| `"block"` | The caller's own block failed while the Service was yielding to it, and the Service did not rescue it (B-24). The failure is the caller's, so it carries no host detail: a caller holding the exception it raised continues that one, and a caller whose block value was refused instead raises under the class the message names |

These names are stable and reserved across kobako releases, and existing type semantics are never modified in place. The set is open: a receiver that meets a `type` it predates reads it as `"undefined"` — the value that attributes nothing a newer one might contradict — and a Fault field it predates is skipped, so a peer built against an earlier contract still delivers the refusal rather than failing the exchange.

---

## Outcome Envelope

The outcome envelope carries the final result of an entire invocation (`#eval` source's last expression or `#run` entrypoint's `#call` return value, or a top-level execution failure). It is distinct from a per-dispatch Reply: it is written by the guest at the end of the invocation export (`__kobako_eval` or `__kobako_run`) and retrieved by the host via `__kobako_take_outcome` after that export returns.

The outcome envelope has two variants:

| Variant | Meaning |
|---------|---------|
| **ok** | The invocation completed without an uncaught top-level exception. Carries the serialized return value — the last mruby expression of `#eval`'s source, or the entrypoint's `#call` return for `#run`. The Host App reads the deserialized Ruby value as the run's `Execution#value`. |
| **panic** | The invocation terminated with an uncaught top-level exception. Carries `origin`, `name`, `message`, `backtrace`, and `available` fields. The host reads `origin` to determine attribution: `origin="service"` maps to `Kobako::ServiceError`, and every other value maps to `Kobako::SandboxError`, so an origin the contract does not reserve attributes to the sandbox rather than widening what a Service can claim. `available` lists the names the invocation could have used in place of the one it named — the top-level constants an undefined `#run` entrypoint failed to resolve against (E-27) — and is empty for a failure that offers no correction. Every field is typed at the envelope layer; a Panic carries no codec-encoded content. |

Outcome bytes the host cannot frame as one of the two variants — an absent outcome included — are a wire-violation signal and raise `Kobako::TrapError` (the fallback path when the guest runtime is structurally corrupted): a message the host cannot frame leaves nothing to attribute a failure to. Guest stdout and stderr do not participate in attribution — they are always captured separately and exposed via the run's `Execution#stdout` / `Execution#stderr`.

---

## Yield Round-Trip

When a Service method invokes `yield` or `block.call` (B-24) on the Yielder materialised from a Call with `block_given=true`, the Host Gem re-enters the Guest Binary synchronously to execute the block body. This is the reverse-direction Call/Reply pair: the host issues the Call, the guest answers with the Reply.

- **Initiator**: the Host Gem (specifically, the Yielder passed to the Service method) issues the Yield Call of every yield round-trip.
- **Responder**: the Guest Binary receives the yield arguments, executes the block body inside the current dispatch frame, and returns a Yield Reply to the host before the re-entry frame exits.
- **Synchronicity**: every yield round-trip is fully synchronous and nests strictly within the dispatch frame that produced the Yielder. From the Service method's perspective, `yield` is an ordinary synchronous method call.
- **Scope**: a Yielder is valid only for the duration of the dispatch frame that produced it. Invoking it after that frame returns raises (E-23).
- **Nesting**: dispatch frames stack in LIFO order; each frame holds at most one Yielder, and nested frames have independent Yielders (B-28). The wasm stack budget bounds nesting depth.

---

## Yield Reply Envelope

The Yield Reply envelope carries the outcome of a single yield round-trip from the Guest Binary back to the host yield site. It is distinct from both the dispatch Reply and the Outcome (per-invocation result): it appears only mid-dispatch, inside the host-issued yield re-entry, and it carries a `break` outcome the dispatch Reply has no counterpart for.

The envelope is a tag-prefixed binary structure: a single byte tag followed by an optional MessagePack payload.

| Tag | Variant | Payload | Meaning |
|-----|---------|---------|---------|
| `0x01` | **ok** | wire-legal value | The block body completed normally. `payload` is the block's last expression value (or the value supplied to `next val`). The host yield expression returns this value to the Service method. |
| `0x02` | **break** | wire-legal value | The block executed `break val` from a non-lambda, non-orphan context. The host yield site terminates the Service method's invocation with `payload` as the effective return value (B-25). |
| `0x03` | RESERVED | — | Reserved tag value. Either endpoint rejects this tag as a wire violation. |
| `0x04` | **error** | Error Record `{class, message, backtrace}` | The block raised an exception, returned a value with no wire representation (E-22), used `return` from a non-lambda block (E-21), or invoked an escaped Yielder (E-23). The host yield site re-raises a Ruby exception with the named class and message. |

The `0x01` ok payload follows the same type mapping as any Reply success value (→ [`docs/wire/payload-msgpack.md`](wire/payload-msgpack.md) § Type Mapping). Capability Handle references (ext 0x01) are legal in the payload position; because host code consumes the ok value, the host restores them to their original objects before the `yield` expression returns (→ [`docs/behavior/dispatch.md`](behavior/dispatch.md) § B-37).

The `0x02` break payload carries the value supplied to `break`. The Host Gem unwinds the Service method's invocation, presenting `payload` to the guest dispatch site as the Service method's return value. A Capability Handle here is **not** restored — the value returns to the guest, not to host code, so it rides back unchanged on the same ID (§ B-37 Notes).

The `0x04` error variant carries an **Error Record** — the same three fields a Panic reports a failure with, so the two failure channels share one shape and the host re-raises from either without consulting the payload codec:

| Field | Type | Meaning |
|-------|------|---------|
| `name` | text | The error's name, re-raised as a class on a host whose language has them (e.g. `"LocalJumpError"`, `"TypeError"`, `"RuntimeError"`) |
| `message` | text | Human-readable description |
| `backtrace` | list of text | mruby backtrace; each element is one line |

A zero-length Yield Reply or any tag outside `{0x01, 0x02, 0x04}` is a wire violation. The host walks the trap path and raises `Kobako::TrapError`.

---

## ABI-Versioned Contract

The Wire Spec is pinned by a single u32 ABI version (→ [`docs/wire-codec.md`](wire-codec.md) § ABI Version): the Guest Binary reports the version it was built against via `__kobako_abi_version`, and the Host Gem accepts it at Sandbox construction only on equality (B-40, E-42). A running sandbox is short-lived (instantiated per invocation, retired after the outcome is retrieved), so there are no long-lived cross-version connections and no stored wire payloads that outlast an ABI version.

Consequently:

- **No in-band version field**: the wire envelopes do not carry a version number or negotiation field. Version alignment is enforced once at Sandbox construction, not at the message level.
- **No negotiation mechanism**: there is no handshake, capability advertisement, or version dispatch. Each side implements exactly one wire shape — the one its ABI version names.
- **Evolution path**: adding, removing, or changing field semantics increments the ABI version and updates both the Host Gem and the bundled Guest Binary simultaneously; an independently-built Guest Binary conforms by rebuilding against the new version. One-sided evolution is not permitted. Release notes and CHANGELOG document wire-affecting changes under Breaking Changes.

---

## Wire-Symmetric Peers

The payload codec has two independent implementations; the core envelope has one, shared by both sides.

| Layer | Host | Guest | Cross-check |
|-------|------|-------|-------------|
| Core envelope | `crates/kobako-transport` | `crates/kobako-transport` | Golden vectors against [`wire/envelope.md`](wire/envelope.md) |
| Payload codec | `lib/kobako/` | `crates/kobako-codec` | Cross-language (Ruby ↔ Rust) |

The two layers differ because ambiguity does. The type mapping — the 11 wire types, the two ext codes, the str/bin rules, the Symbol-keyed `kwargs` — is where two languages' type systems and encoding conventions disagree, so it earns a second implementation and a fuzz harness. The envelope asks its implementers to agree on three routing fields and a byte string, and it is the fixed tier every assembly composes against: one definition is the guarantee there, and the layout document is what holds it honest. Every envelope this document specifies exists as a wire-codable type in `kobako-transport`.

The two payload peers are held to each other over bytes by the round-trip fuzz: generated values cross both implementations and the re-encoding must come back byte-identical, so the type mapping and both ext codes are checked one against the other over every shape the harness produces.

The guest's own value walk — mruby values to and from the payload codec's types — sits below both peers rather than beside them, naming no payload type of its own. What it can get wrong is therefore a value's fidelity, not a type's shape, and it has no peer to differ against; it is held instead to an identity law, driving the real Guest Binary so that a value the host puts on the wire comes back the value it went in as.

A shape the harness never produces sits outside that reach: a payload type one peer grows and the other does not stays invisible until a generated case reaches it. `rake gate:wire:symmetry` closes that by comparing the two peers' wire-codable type names. A name present on one side only must hold an entry under Accepted asymmetries, each carrying the reason the divergence is the contract's own shape rather than drift; an entry the inventories no longer diverge on is itself a violation to drop. An empty block is the target state.

One standing divergence lives outside the inventory comparison: success and failure are a value on the guest (`Outcome`) but return-or-raise on the host. It is a difference in what each side's language makes idiomatic, not in what the wire carries, so the inventories stay comparable without it.

### Accepted asymmetries

The ledger below carries type names across the two payload peers, which is the
granularity the gate reads. Field names inside a type are below it: a peer may
spell a field whatever its language makes idiomatic, since the wire position is
what the contract fixes.

```
```
