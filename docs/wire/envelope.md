# Core Envelope

This document pins the byte layout of the **core envelope** — the outer frame of every host↔guest message. The core envelope is a fixed layout, not MessagePack: it carries only what routing and outcome attribution need, and hands everything else through as an opaque `payload` the payload adapter owns (→ [`payload-msgpack.md`](payload-msgpack.md) for the default adapter).

`docs/wire-codec.md` is the anchor that relates the two layers and holds the ABI surface; this document is the core layer's byte-level reference. The abstract shape it encodes is specified in [`../wire-contract.md`](../wire-contract.md).

Both the Host Gem's native side (`crates/kobako-runtime`) and the Guest Binary (`crates/kobako-codec`) implement this layout independently. Byte values stated here are fixed for the life of an ABI version (→ `docs/wire-codec.md` § ABI Version).

---

## Properties of this layer

Two properties define the core envelope, and every layout below satisfies both.

- **Decodable without the payload adapter.** A side resolves a Call's target, names its method, learns whether a Reply succeeded, and attributes a failed invocation using only the fields here. It never interprets a payload byte to do so. This is what makes the adapter replaceable: two endpoints sharing a protobuf schema carry no MessagePack dependency at all.
- **Non-recursive.** Every field is a scalar, a byte string, or a flat list. The layer has no nesting to bound and cannot overflow a stack on untrusted input.

---

## Primitives

Every core envelope is built from four primitives. All integers are unsigned big-endian, matching the invocation-channel frame prefix.

| Primitive | Layout |
|-----------|--------|
| `u8` | one byte |
| `u32` | four bytes, big-endian |
| `bytes` | `u32` length, then exactly that many bytes |
| `list<bytes>` | `u32` count, then that many `bytes` values back to back |

A `bytes` field of length `0` is a present, empty value. Fields that mean "absent" say so explicitly in their table row.

### Framing rule

**Every field except the last is self-delimiting; the last field consumes the remainder of the message.**

The last field of an envelope that carries one is always its `payload` or `body` — the part whose length the transport already knows, from the invocation-channel frame prefix or from the ABI's `len` return. Repeating that length inside the envelope would give two sources for one fact, so the envelope does not carry it.

The rule applies recursively: an envelope nested as another's trailing field (a Panic inside an Outcome, an Error Record inside a Yield Reply) inherits the remainder as its own extent.

Consequently a decode either consumes the message exactly or fails. A `u32` length that overruns the message end, a `list<bytes>` count the message cannot satisfy, or bytes left over after a message whose last field is self-delimiting are all wire violations; the receiving side rejects rather than ignoring the excess, so a framing desync fails loudly instead of silently dropping data.

---

## Call

The guest→host dispatch Call, and the shape every reverse-direction Call is measured against.

| Field | Type | Meaning |
|-------|------|---------|
| `kind` | `u8` | `0` — `target` is a bound constant's path; `1` — `target` is a Capability Handle reference. No other value is legal. |
| `target` | `bytes` when `kind=0`, `u32` when `kind=1` | The constant path as UTF-8 (`"MyService::KV"`, `"File"`), or the Handle ID. |
| `method` | `bytes` | The method name as UTF-8. One method per Call. |
| `block_given` | `u8` | `0` or `1`. No other value is legal. |
| `payload` | remainder | The invocation arguments, encoded by the payload adapter. Opaque to this layer. |

The two `target` forms are discriminated by the explicit `kind` tag rather than by the shape of `target` itself, so a side reads the routing fields without consulting any encoding but this one.

A Handle ID of `0` is the invalid sentinel and is a wire violation in the `target` position; the maximum valid ID is `0x7fff_ffff` (→ [`../wire-contract.md`](../wire-contract.md) § Capability Handle).

---

## Reply

The answer to one dispatch Call.

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | `0` — success; `1` — fault. No other value is legal. |
| `body` | remainder | `tag=0`: the return value, encoded by the payload adapter. `tag=1`: the fault, encoded by the payload adapter (→ [`payload-msgpack.md`](payload-msgpack.md) § ext 0x02). |

Success-versus-fault is decided at this layer, not inside the payload: a guest learns whether the Service returned or raised by reading one byte, whatever schema the payload carries. That is why the fault rides its own arm rather than a reserved payload value.

---

## Yield Call and Yield Reply

The reverse-direction pair, nested inside the dispatch frame the host is still answering.

**Yield Call** is the payload alone — the block's yield arguments, encoded by the payload adapter. The ABI's `req_len` frames it (→ `docs/wire-codec.md` § ABI Signatures), so no length prefix is repeated.

**Yield Reply**:

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | `0x01` ok · `0x02` break · `0x04` error. `0x03` is reserved and rejected by both sides; so is any other value. |
| `body` | remainder | `tag` `0x01` / `0x02`: the block's value or the `break` value, encoded by the payload adapter. `tag` `0x04`: an Error Record. |

A zero-length Yield Reply is a wire violation.

### Error Record

The guest's report that something it was running raised. A block failure and an invocation failure share this layout, and the host re-raises from these fields without consulting the payload adapter.

It is distinct from a Fault, which travels the other way: a Fault is the host refusing or failing a Call, categorized by one of three reserved `type` values the guest maps to a proxy-side error, and it lives in the payload (→ [`payload-msgpack.md`](payload-msgpack.md) § ext 0x02). An Error Record names a guest-side exception class verbatim and carries no category.

| Field | Type | Meaning |
|-------|------|---------|
| `class` | `bytes` | Exception class name as UTF-8 (`"LocalJumpError"`, `"RuntimeError"`). |
| `message` | `bytes` | Human-readable description as UTF-8. |
| `backtrace` | `list<bytes>` | mruby backtrace, one UTF-8 line per element. An empty list is legal. |

---

## Outcome

The per-invocation final result, written to OUTCOME_BUFFER by the invocation export and read by the host through `__kobako_take_outcome`.

| Field | Type | Meaning |
|-------|------|---------|
| `tag` | `u8` | `0x01` — a Result follows; `0x02` — a Panic follows. No other value is legal. |
| `body` | remainder | `tag=0x01`: the invocation's value, encoded by the payload adapter. `tag=0x02`: a Panic. |

A Result is the value alone — the `tag` already discriminates the variant, so no further framing is added.

A zero-length OUTCOME_BUFFER or any other tag is a wire violation; the host raises `Kobako::TrapError`.

### Panic

The Error Record plus the two fields attribution needs.

| Field | Type | Meaning |
|-------|------|---------|
| `origin` | `bytes` | `"sandbox"` (mruby script error or boot fault) or `"service"` (unrescued Service failure) as UTF-8. An unrecognized value attributes as `"sandbox"`. |
| `class` | `bytes` | Exception class name as UTF-8. |
| `message` | `bytes` | Exception message as UTF-8. |
| `backtrace` | `list<bytes>` | mruby backtrace, one UTF-8 line per element. |
| `details` | remainder | Structured diagnostics, encoded by the payload adapter. An empty remainder means absent. |

Attribution reads `origin` at this layer: `"service"` maps to `Kobako::ServiceError`, anything else to `Kobako::SandboxError` (→ [`../behavior/errors.md`](../behavior/errors.md)). A host therefore attributes a failed invocation without decoding a payload byte, and `details` stays optional supplementary information rather than a field attribution depends on.

---

## Run

The host→guest entrypoint dispatch envelope, delivered on the command buffer to `__kobako_run`.

| Field | Type | Meaning |
|-------|------|---------|
| `entrypoint` | `bytes` | The top-level constant name as UTF-8, matching `/\A[A-Z]\w*\z/`. The host normalizes a String argument before encoding. |
| `payload` | remainder | The entrypoint's arguments, encoded by the payload adapter. |

Run is the reverse-direction sibling of Call: `entrypoint` routes it, the payload feeds it. It carries no `method` — the entrypoint is invoked through its own `#call` — and no `block_given`, because `#run` supplies no block.

---

## Invocation Frames

The two length-prefixed stdin frames every invocation entry point consumes (→ `docs/wire-codec.md` § ABI Signatures). Each frame's own `[u32 be][bytes]` channel prefix is the transport's, not part of these layouts.

### Frame 1 — preamble

| Field | Type | Meaning |
|-------|------|---------|
| `paths` | `list<bytes>` | Each bound constant's path as UTF-8. An empty list is legal and means no Service is bound. |

### Frame 3 — snippets

| Field | Type | Meaning |
|-------|------|---------|
| `count` | `u32` | Number of entries that follow, in insertion order. `0` is legal. |
| per entry `kind` | `u8` | `0` — mruby source; `1` — RITE bytecode. No other value is legal. |
| per entry `name` | `bytes` | Present only when `kind=0`: the filename the guest compiles under, reported in backtraces as `(snippet:<name>)`. |
| per entry `body` | `bytes` | UTF-8 mruby source when `kind=0`; RITE bytecode when `kind=1`. |

A bytecode entry carries no `name`: the snippet's filename, when present, is read from the bytecode's embedded `debug_info` section at load time, and bytecode omitting `debug_info` is a legal payload.

Frame 2 — the `#eval` user source — is raw UTF-8 bytes with no envelope of its own.

---

## Size and Depth Bounds

**16 MiB per dispatch, applied to the whole envelope** in either direction, not to the payload alone. A side checks the bound before allocating, so an oversized message is a wire violation rather than an allocation the receiver has to survive.

**Nesting bounds belong to the payload adapter.** This layer is non-recursive (→ § Properties of this layer), so it has no depth to budget. The adapter budgets depth per document, and the envelope and each payload it carries are separate documents with separate budgets (→ [`payload-msgpack.md`](payload-msgpack.md) § Structural Nesting Depth).
