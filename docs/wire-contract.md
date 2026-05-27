# Wire Contract

This document specifies the abstract logical shape of every message exchanged between the Host Gem and the Guest Binary during a Sandbox invocation (`#eval` or `#run`). It is a **Consistency-layer contract**: both sides implement it independently, and a kobako gem release ships exactly one version of it. Byte-level encoding (msgpack type mapping, ext code numbers, binary layout) is specified in [`docs/wire-codec.md`](wire-codec.md).

The governing summary of this contract lives in `SPEC.md` § Wire Contract; this document is its abstract reference.

---

## Transport Role

- **Initiator**: the Guest Binary (`Kobako::Transport::Proxy`) is the sole initiator of all host↔guest communication. The Host Gem never pushes messages to the guest unprompted.
- **Responder**: the Host Gem handles each request synchronously within the same Wasm import function call frame, then returns the response to the guest before that frame exits.
- **Synchronicity**: every Transport round-trip is fully synchronous. From the guest mruby script's perspective, a Service method call is an ordinary synchronous function call that completes before the next line executes. There are no callbacks, promises, or yield-resume mechanisms.
- **Medium**: Wasm linear memory. The guest writes the serialized Request into linear memory and calls a Wasm import function; the host reads and writes through a memory view provided by the Wasm engine. This is an implementation note; the wire contract specifies message shape, not transport mechanics.

---

## Request Shape

Every host↔guest Transport request carries exactly five logical fields:

| Field | Type | Meaning |
|-------|------|---------|
| `target` | Member path (two-level string `"Namespace::Member"`) **or** Capability Handle reference | Identifies the Ruby object that receives the call. The two forms are distinguishable on the wire without inspecting `method` or `args`. |
| `method` | string | The single method name to invoke on the resolved target via `public_send`. One method per Request; no multi-segment traversal in a single wire call. |
| `args` | ordered list | Positional arguments passed to the method. Elements may themselves be Capability Handle references. |
| `kwargs` | key-value map | Keyword arguments passed to the method. Keys are Symbols on the wire (→ [`docs/wire-codec.md`](wire-codec.md) § Ext Types → ext 0x00); the host passes them to dispatch unchanged. An empty kwargs map is always present (never absent) to keep field positions stable. |
| `block_given` | bool | Whether the guest call site supplied a block. When `true`, the Host Gem materialises a Yielder and passes it to the resolved Service method as `&block` (B-23). When `false`, the Service method receives no block and `block_given?` returns `false`. The block body itself is never serialized — only this flag travels on the wire; the block remains inside the Guest Binary and is invoked through Yield Round-Trip. |

The `target` string form uses Ruby constant-path syntax (`"Namespace::Member"`) so the wire value is identical to the guest-side constant access expression — no cognitive translation between layers.

---

## Response Shape

Every Response carries one of two mutually exclusive variants:

| Variant | Fields | Meaning |
|---------|--------|---------|
| **Success** | `status=0`, `value` | The call completed successfully. `value` carries the return value (a primitive or a Capability Handle reference). |
| **Fault** | `status=1`, fault envelope | The call failed. The fault envelope (see Fault Envelope below) describes the failure category and message. |

A Response always matches exactly one variant. There is no partial success or streaming response.

---

## Capability Handle

A **Capability Handle** is an opaque token used on either side of the wire to reference a stateful Ruby object that is not directly wire-representable (e.g., a session, connection, `StringIO`, custom Env / Context class). The abstract contract is:

- **Opaque**: the guest receives a Handle token and cannot extract the underlying Ruby object from it; the only permitted operation is passing the token back as a `target` or `args` element in a subsequent Request, or invoking methods on it which dispatch as Transport requests.
- **Host-allocated**: the wire layer on the host side allocates a Handle automatically in two symmetric situations — whenever a Service method returns a stateful object (host→guest return path, → [`docs/behavior.md`](behavior.md) § B-14), and whenever `#run` is invoked with arguments containing non-wire-representable objects (host→guest argument path, → [`docs/behavior.md`](behavior.md) § B-34). The Host App has no API to create or inspect Handles directly.
- **Scoped to a single invocation**: a Handle token issued during invocation N is invalid in invocation N+1. The Catalog::Handles is fully reset at the start of every invocation (`#eval` or `#run`); the reset is uniform regardless of allocation source.
- **Not constructible by guest or Host App**: neither the guest mruby API nor the Host App API exposes a public constructor that converts a bare integer to a Handle. A raw integer presented as a Handle on the wire is rejected before it reaches the Catalog::Handles; a `Kobako::Handle` instance fabricated through any non-public path on the host side is rejected at `#run` host pre-flight. Handle allocation is exclusively internal to the Host Gem's wire layer.
- **ID cap**: the opaque ID component of a Handle is bounded by `0x7fff_ffff` (2³¹ − 1). Allocation beyond this cap raises `Kobako::HandlerExhaustedError` immediately (fail-fast; no silent wraparound).

Byte-level encoding of the Capability Handle (ext type number, binary layout) is specified in [`docs/wire-codec.md`](wire-codec.md).

---

## Fault Envelope

The fault envelope appears inside a Response `status=1` variant and describes a Service-layer failure. Maps to the Ruby value object `Kobako::Fault`. It carries three fields:

| Field | Type | Meaning |
|-------|------|---------|
| `type` | string | One of the three reserved error type names (see table below). Identifies the failure category. |
| `message` | string | Human-readable description of the failure. |
| `details` | any (optional) | Structured supplementary information. Omitted or null when not present. |

The three reserved `type` values are:

| `type` value | Failure it represents |
|---|---|
| `"runtime"` | A general Ruby exception raised inside a Service method during dispatch |
| `"argument"` | Argument parsing failed, or the method name does not exist on the target (`NoMethodError`) |
| `"undefined"` | The `target` string path does not match any registered Member, or the `target` Handle ID does not exist in the current invocation's Catalog::Handles |

These three names are stable and reserved across kobako releases. Adding a new `type` value requires a kobako gem release that updates both host and guest codec implementations simultaneously; existing type semantics are never modified in place.

---

## Outcome Envelope

The outcome envelope carries the final result of an entire invocation (`#eval` source's last expression or `#run` entrypoint's `#call` return value, or a top-level execution failure). It is distinct from the per-dispatch Response: it is written by the guest at the end of the invocation export (`__kobako_eval` or `__kobako_run`) and retrieved by the host via `__kobako_take_outcome` after that export returns.

The outcome envelope has two variants:

| Variant | Meaning |
|---------|---------|
| **Result envelope** | The invocation completed without an uncaught top-level exception. Carries the serialized return value — the last mruby expression of `#eval`'s source, or the entrypoint's `#call` return for `#run`. The invocation returns the deserialized Ruby value to the Host App. |
| **Panic envelope** | The invocation terminated with an uncaught top-level exception. Carries `origin`, `class`, `message`, `backtrace`, and optional `details` fields. The host reads `origin` to determine attribution: `origin="service"` maps to `Kobako::ServiceError`; `origin="sandbox"` or absent maps to `Kobako::SandboxError`. `details` carries optional structured diagnostics (e.g., the available top-level constant list for an undefined `#run` entrypoint, E-27). |

The host reads zero-length outcome bytes or an unrecognized envelope tag as a wire-violation signal and raises `Kobako::TrapError` (the fallback path when the guest runtime is structurally corrupted). Guest stdout and stderr do not participate in attribution — they are always captured separately and exposed via `Sandbox#stdout` / `Sandbox#stderr`.

---

## Yield Round-Trip

When a Service method invokes `yield` or `block.call` (B-24) on the Yielder materialised from a Request with `block_given=true`, the Host Gem re-enters the Guest Binary synchronously to execute the block body. This is the symmetric counterpart of a Request/Response dispatch: the host initiates, the guest responds.

- **Initiator**: the Host Gem (specifically, the Yielder passed to the Service method) is the initiator of every yield round-trip.
- **Responder**: the Guest Binary receives the yield arguments, executes the block body inside the current dispatch frame, and returns a YieldResponse to the host before the re-entry frame exits.
- **Synchronicity**: every yield round-trip is fully synchronous and nests strictly within the dispatch frame that produced the Yielder. From the Service method's perspective, `yield` is an ordinary synchronous method call.
- **Scope**: a Yielder is valid only for the duration of the dispatch frame that produced it. Invoking it after that frame returns raises (E-23).
- **Nesting**: dispatch frames stack in LIFO order; each frame holds at most one Yielder, and nested frames have independent Yielders (B-28). The wasm stack budget bounds nesting depth.

---

## YieldResponse Envelope

The YieldResponse envelope carries the outcome of a single yield round-trip from the Guest Binary back to the host yield site. It is distinct from both Response (per-dispatch reply) and Outcome (per-invocation result): it appears only mid-dispatch, inside the host-initiated yield re-entry.

The envelope is a tag-prefixed binary structure: a single byte tag followed by an optional MessagePack payload.

| Tag | Variant | Payload | Meaning |
|-----|---------|---------|---------|
| `0x01` | **ok** | wire-legal value | The block body completed normally. `payload` is the block's last expression value (or the value supplied to `next val`). The host yield expression returns this value to the Service method. |
| `0x02` | **break** | wire-legal value | The block executed `break val` from a non-lambda, non-orphan context. The host yield site terminates the Service method's invocation with `payload` as the effective return value (B-25). |
| `0x03` | RESERVED | — | Reserved tag value. Receivers reject this tag as a wire violation. |
| `0x04` | **error** | map `{class, message, backtrace}` | The block raised an exception, returned a value with no wire representation (E-22), used `return` from a non-lambda block (E-21), or invoked an escaped Yielder (E-23). The host yield site re-raises a Ruby exception with the named class and message. |

The `0x01` ok payload follows the same wire type mapping as any Response success value (→ [`docs/wire-codec.md`](wire-codec.md) § Type Mapping). Capability Handle references (ext 0x01) are legal in the payload position; because host code consumes the ok value, the host restores them to their original objects before the `yield` expression returns (→ [`docs/behavior.md`](behavior.md) § B-37).

The `0x02` break payload carries the value supplied to `break`. The Host Gem unwinds the Service method's invocation, presenting `payload` to the guest dispatch site as the Service method's return value. A Capability Handle here is **not** restored — the value returns to the guest, not to host code, so it rides back unchanged on the same ID (§ B-37 Notes).

The `0x04` error payload is a MessagePack map with three keys:

| Key | Value type | Meaning |
|-----|-----------|---------|
| `"class"` | str | Exception class name to re-raise on the host (e.g. `"LocalJumpError"`, `"TypeError"`, `"RuntimeError"`) |
| `"message"` | str | Human-readable description |
| `"backtrace"` | array of str | mruby backtrace; each element is one line |

A zero-length YieldResponse or any tag outside `{0x01, 0x02, 0x04}` is a wire violation. The host walks the trap path and raises `Kobako::TrapError`.

---

## Release-Internal Contract

The Wire Spec is a **release-internal contract**: the Host Gem and Guest Binary ship together in a single kobako gem release and are always version-coupled. A running sandbox is short-lived (instantiated per invocation, retired after the outcome is retrieved), so there are no long-lived cross-version connections and no stored wire payloads that outlast a release.

Consequently:

- **No in-band version field**: the wire envelope does not carry a version number or negotiation field. Version alignment is enforced at the gem release boundary, not at the message level.
- **No negotiation mechanism**: there is no handshake, capability advertisement, or version dispatch. The single wire shape defined in this release is the only shape either side implements.
- **Evolution path**: adding, removing, or changing field semantics requires a kobako gem release that updates both host and guest implementations simultaneously. One-sided evolution is not permitted. Release notes and CHANGELOG document wire-affecting changes at the release boundary.
