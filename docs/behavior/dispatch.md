# Dispatch — Transport calls and Handle lifecycle

Guest→host Transport dispatch, the `Catalog::Handles` lifecycle, `#run` argument auto-wrap, and Handle restoration across the boundary. The governing summary lives in [`SPEC.md`](../../SPEC.md)
§ Behavior; this file is the per-anchor reference. `B-xx` anchors are global
and append-only across the corpus (N-8).

## B-12 — Guest-initiated Transport dispatch to a bound Ruby object

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. A Member is bound at `MyService::KV` (e.g., `MyService::KV`). The guest holds a reference to the constant `MyService::KV` and calls a method on it. |
| **Operation** | Guest code executes `MyService::KV.method_name(arg1, arg2, key: value)` — a synchronous method call from within the mruby script. |
| **Result / Final State** | The Host Gem resolves the target to the Ruby object bound at `MyService::KV` and invokes `object.public_send(:method_name, arg1, arg2, key: value)`. The Ruby return value is serialized and returned to the guest as the synchronous result of the call — from the guest's perspective, the call completes as an ordinary synchronous Ruby method invocation. Each dispatch invokes the bound object's method exactly once. Keyword argument names travel on the wire as Symbols (→ [`docs/wire-codec.md`](../wire-codec.md) § Type Mapping) and reach `public_send` without further conversion. A positional argument or kwargs value with no wire representation is rejected at the guest dispatch call site rather than coerced to an `Object#to_s` string (E-55). An unresolved target path surfaces per E-12; a method name that resolves to Ruby's ambient reflection / eval surface is rejected before dispatch (B-42). |

---

## B-13 — Service method returns a primitive value

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated Transport dispatch (B-12) has been dispatched. The bound Ruby object's method returns a value that is **wire-representable**: `nil`, Boolean, Integer, Float, String, binary String, Symbol, Array, or Hash. |
| **Operation** | The Host Gem's wire codec serializes the return value and delivers it to the guest as the dispatch response. |
| **Result / Final State** | The guest receives the return value as the synchronous result of the method call, deserialized to the corresponding mruby type. The value is indistinguishable from a locally computed mruby value. No entry is added to the Catalog::Handles. A value is **wire-representable** when it is `nil`, a Boolean, Integer, Float, String, binary String, Symbol, an Array of wire-representable values, a Hash with wire-representable keys and values, or another `Kobako::Handle`; such collections are transmitted in full by value. A return value outside this set takes the Handle allocation path (B-14). |

---

## B-14 — Service method returns a stateful object (Host-side Handle allocation)

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated Transport dispatch (B-12) has been dispatched. The bound Ruby object's method returns a Ruby object that is not wire-representable — for example, a session object, a connection, or any stateful host resource. |
| **Operation** | A return value is routed through the Handle allocation path if and only if its type is not wire-representable per the definition in B-13. The wire layer then automatically registers the object in the Sandbox's Catalog::Handles. |
| **Result / Final State** | The host-side object is stored in `Catalog::Handles` under a new opaque u32 Handle ID. The guest receives a Capability Handle (an opaque integer token) as the dispatch response value, not the object itself. The guest can pass this Handle as the `target` in subsequent dispatch requests to invoke methods on the same host-side object. The Host App has no API to create or inspect Handles directly — Handle allocation is an internal wire-layer operation. Handle lifecycle — per-invocation scope and ID limits — is specified in B-15..B-21; the symmetric host→guest direction (`#run` arguments) routes through the same `Catalog::Handles` allocator under B-34. |

---

## B-15 — Handle ID is allocated with a monotonically increasing counter scoped to a single invocation

| Field | Value |
|-------|-------|
| **Initial State** | An invocation (`#eval` or `#run`) has just begun. The Catalog::Handles counter is reset to 1. No entries exist in the table. |
| **Operation** | The Host Gem's wire layer allocates a new Handle, either for a stateful return value from a Service method (B-14) or for a non-wire-representable argument supplied to `#run` (B-34). |
| **Result / Final State** | The first Handle issued in this invocation receives ID 1, the second ID 2, and so on. IDs are assigned in allocation order and are unique within the invocation; the counter never wraps or reuses an ID — when the cap is reached, allocation fails (see B-21). ID 0 is reserved as the invalid sentinel; allocation never returns 0. |

---

## B-16 — Guest passes a previously-received Handle as an argument to a Service dispatch

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress. The guest holds a `Kobako::Handle` (mruby object) obtained from a prior dispatch response (B-14) or from a `#run` argument auto-wrapped by the host (B-34) in the same invocation. The Handle's internal ID resolves to a live entry in `Catalog::Handles`. |
| **Operation** | Guest code invokes a method on a Member and passes the `Kobako::Handle` — as a positional argument (e.g., `Store.put(handle, value)`), as a keyword-argument value (e.g., `Http.get(url, cred: handle)`), nested inside an Array or Hash argument (e.g., `Store.put_all([handle_a, handle_b])` or `Http.post(headers: { auth: handle })`), or any mix of these in one call. |
| **Result / Final State** | The Host Gem walks each positional and keyword argument — descending into Array elements and Hash keys and values one structural level at a time — and replaces every `ext 0x01` Capability Handle with the live host object its ID resolves to in the Catalog::Handles. Positional, keyword, and nested Handle arguments resolve identically, symmetric with the guest→host return path (B-37): every Handle resolves back to its host object before the call reaches `public_send`, while wire-representable leaves pass through unchanged with all surrounding Array / Hash structure preserved. The Service method receives actual Ruby objects, not IDs or tokens. The method executes and its return value follows the normal primitive (B-13) or stateful (B-14) path. The guest holds a `Kobako::Handle` mruby proxy, never the raw integer ID; an ID with no live entry surfaces per E-13. |

---

## B-17 — Chained composition: Handle returned by Service A used as target in a subsequent call to Service B

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress. Service A has been called via Transport dispatch and returned a stateful object; the guest holds `handle_a` (a `Kobako::Handle` proxy). |
| **Operation** | Guest code calls a method directly on `handle_a` (e.g., `handle_a.find(id)`), using the Handle as the dispatch target. The wire layer encodes `handle_a` as the `target` field of the Request. |
| **Result / Final State** | The Host Gem resolves `handle_a`'s ID against `Catalog::Handles` and invokes `public_send(:find, id)` on the host-side Ruby object that `handle_a` represents. If that call returns another stateful object, a new Handle `handle_b` is allocated and returned to the guest. Each step in the chain is an independent, synchronous dispatch; there is no implicit multi-hop traversal within a single wire call. Chain depth is unbounded within a single invocation as long as each step produces a Handle that survives to the next call. Every host object reachable by the guest entered through an explicit allocation path — Service-returned (B-14) or `#run` argument auto-wrapped (B-34); there is no implicit intermediate binding path. |

---

## B-18 — Handle issued in invocation N is presented as a target in invocation N+1

| Field | Value |
|-------|-------|
| **Initial State** | Invocation N has completed. The guest code attempts to retain a Handle ID from invocation N and presents it as the `target` in a new invocation (N+1, of either verb). At the start of invocation N+1 the Catalog::Handles has been fully reset: all entries from invocation N (both Service-returned via B-14 and host-injected via B-34) are cleared and the counter restarted. |
| **Operation** | Guest code in invocation N+1 calls a method using the stale Handle ID as the dispatch target. |
| **Result / Final State** | The `Catalog::Handles` lookup for that ID returns `:undefined` — the ID does not exist in the fresh table. The stale Handle is invalid; the Host Gem treats this as an unrecognized target. The error path is E-13. Invocation N+1 is not interrupted for other dispatch requests that do not reference stale IDs. This outcome is unconditional: no Handle survives the invocation boundary regardless of how it was allocated (B-14 or B-34), even when N and N+1 execute the same source or entrypoint with the same Service bindings. |

---

## B-19 — Sandbox is discarded: all Handles for that Sandbox become invalid

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Sandbox` instance exists with zero or more completed invocations. The Catalog::Handles is owned by this Sandbox instance. |
| **Operation** | The Sandbox instance is garbage-collected or goes out of scope; Ruby reclaims it. |
| **Result / Final State** | The Catalog::Handles and all its entries are destroyed as part of Sandbox teardown. Every Handle that was issued during any invocation on this Sandbox is permanently invalid. No Handle entry is shared with, transferred to, or reachable from any other Sandbox instance. Handles are not reference-counted and there is no explicit `release` API for individual entries; validity is scoped to the owning Sandbox and the issuing invocation (B-18), uniformly across allocation sources (B-14 / B-34). |

---

## B-20 — Neither guest nor Host App can construct or dereference a Handle from a raw integer

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress (or about to begin via `#run`). Either the guest mruby has access to an arbitrary integer value (e.g., `42` or any computed integer), or the Host App holds an arbitrary integer it intends to present as a Handle. |
| **Operation** | Code on either side of the boundary attempts to use a raw integer as a Handle — for example, by constructing a `Kobako::Handle`-like object from an integer literal, or by any means other than receiving a Handle from a Service dispatch response (B-14) or from a `#run` host-side auto-wrap (B-34). |
| **Result / Final State** | No valid `Kobako::Handle` object is produced from a bare integer on either side. Neither the guest mruby API nor the Host App API exposes a public constructor that converts an integer to a Handle. A raw integer presented as a dispatch target does not carry the Handle wire encoding (`ext 0x01`); the host-side wire decoder rejects the malformed encoding before the value reaches `Catalog::Handles`. A `Kobako::Handle` instance fabricated on the host side through any non-public path and passed to `#run` raises `ArgumentError` at host pre-flight (E-29). The `Kobako::Handle` class holds the u32 ID internally and does not expose it as a readable integer attribute; each side enforces separately — host pre-flight rejection (E-29) and blocked guest construction (B-39). Handle ID unguessability is not a security property: the capability boundary rests on `Catalog::Handles` membership plus per-invocation isolation (B-18), not on ID secrecy — a guessed or forged ID grants no reference the invocation was not already handed (B-14 / B-34). |

---

## B-21 — Catalog::Handles exhaustion: allocation attempt beyond the ID cap

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress. The Catalog::Handles counter has reached `0x7fff_ffff` (2³¹ − 1), the maximum valid Handle ID. |
| **Operation** | The Host Gem's wire layer attempts to allocate one additional Handle for a new stateful return value. |
| **Result / Final State** | The allocation fails immediately with a `Kobako::HandleExhaustedError` (a `Kobako::SandboxError` subclass). The counter is not incremented, no new entry is written to the Catalog::Handles, and no ID is silently truncated or wrapped. The error is raised to the Host App; the current invocation terminates. |

---

## B-34 — `#run` argument auto-wraps into a host-side Handle when not wire-representable

| Field | Value |
|-------|-------|
| **Initial State** | `#run(target, *args, **kwargs)` is invoked (B-31). At least one element of `args`, or at least one value in `kwargs`, is not wire-representable per the type set defined in B-13 — for example, a `StringIO`, an arbitrary Host App `Env` instance, or any other Ruby object whose class is outside the wire 12-entry mapping. |
| **Operation** | During Invocation envelope encoding the Host Gem walks the `args` Array and the `kwargs` Hash values; container types (Array, Hash) are walked one level at a time, and each leaf value that is not wire-representable is allocated into the Sandbox's Catalog::Handles. The allocator returns a fresh u32 ID (B-15) which is written into the envelope as an `ext 0x01` Capability Handle in place of the original Ruby object. Wire-representable leaves pass through unchanged. The walk wraps Hash values only: a Hash key must already be wire-representable (B-13), since a key cannot cross as a Capability Handle the way a value can, so a non-representable key is rejected with `Kobako::SandboxError` rather than auto-wrapped. |
| **Result / Final State** | The guest mruby code receives a `Kobako::Handle` proxy at each position where the host supplied a non-wire-representable argument. The proxy carries no observable Ruby value content; method calls on it dispatch back to the host through the same `method_missing` → Transport path the guest uses for Service-returned Handles (B-17). The host-side `Catalog::Handles` entry remains live for the duration of the invocation and is cleared together with all other Handles at the invocation boundary (B-18 / B-19). `Catalog::Handles` cap exhaustion during the walk raises `Kobako::HandleExhaustedError` at host pre-call via the same path as B-21 / E-07. The walk traverses Array and Hash containers but does not descend into the instance variables or internal structure of a non-wire-representable leaf — once a leaf is wrapped, its sub-structure is hidden behind the Handle. A `Kobako::Handle` already produced internally by the Host Gem is rejected at host pre-flight (E-29); auto-wrap never re-wraps an existing Handle. |

---

## B-37 — Guest returns a Capability Handle across the boundary (Host-side Handle restoration)

This behavior refines the value-return semantics of B-06 (`#eval`) and B-31 (`#run`), and of the yield-block ok result (B-26), for the case where the returned value is, or contains, a Capability Handle.

| Field | Value |
|-------|-------|
| **Initial State** | The guest hands a value back to host code — either as the invocation result (the last mruby expression of `#eval`, or the entrypoint's `#call` return for `#run`), or as the value a guest block returns to a yielding Service method's `yield` expression (the YieldResponse `0x01` ok payload of B-26). That value is a `Kobako::Handle` proxy the guest received earlier in this same invocation (from a Service return per B-14, or from a `#run` argument auto-wrap per B-34), or an Array / Hash containing one or more such proxies. |
| **Operation** | The guest serializes the value through the same wire codec used for every guest→host value; each `Kobako::Handle` proxy encodes as an `ext 0x01` Capability Handle carrying its Handle ID, which both the Result envelope `value` position and the YieldResponse ok payload position accept. On the host, after decoding the payload, the Host Gem walks the decoded value — descending into Array elements and Hash keys and values one level at a time — and replaces each decoded Capability Handle with the host-side object bound to its ID in this invocation's Catalog::Handles. Wire-representable leaves pass through unchanged. |
| **Result / Final State** | At each position where the guest returned a Handle, the host yields the original host-side object instance — the same object Catalog::Handles holds. For the invocation result, `#eval` / `#run` returns that object (bare, or inside a restored Array / Hash with all other structure preserved). For a yield-block ok result, the Service method's `yield` expression (B-26) receives that object. The Host App and Service code receive ordinary host objects and never observe a `Kobako::Handle`. Restoration is a read against Catalog::Handles, not an allocation: no new Handle ID is issued and the table is unchanged. A `0x02` break value is excluded from restoration — it returns to the guest rather than to host code, riding back unrestored on the same ID (B-25) — and the `0x04` error YieldResponse carries no value to restore (E-22). Because the guest cannot fabricate a Handle (B-20), every Handle it can legitimately return resolves to a live object; a returned ID with no live binding indicates a corrupted runtime and raises `Kobako::SandboxError` through the same wire-violation fallback as a malformed Result value (E-09). |

---

## B-58 — Positional and keyword arguments partition by Ruby 3 call semantics

This behavior refines the argument delivery of B-12 for a dispatch whose call site mixes brace-less keyword syntax with an explicit Hash literal.

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated Transport dispatch (B-12) is in progress. The guest calls a Service or Handle method whose argument list combines positional arguments, a brace-less `key: value` keyword argument, and/or an explicit `{...}` Hash literal in positional position — for example `MyService::KV.store(key, {a: 1}, ttl: 60)`. |
| **Operation** | The guest bridge reads the call's arguments with the keyword arguments kept in their own bucket, distinct from the positional rest, matching the positional/keyword partition Ruby 3 tracks at the call site. |
| **Result / Final State** | A brace-less `key: value` argument reaches the Service as a keyword argument, its name a Symbol (E-15). An explicit `{...}` Hash literal passed in positional position stays a positional argument and is delivered as an ordinary Hash value; it is never folded into the keyword bucket. When the call passes no brace-less keyword, the Service is invoked with no keyword arguments even when the final positional argument is a Hash. The example dispatches as `object.public_send(:store, key, {a: 1}, ttl: 60)` — positional `[key, {a: 1}]`, keyword `{ttl: 60}`. E-55 rejection of a value with no wire representation applies identically to a positional argument and to a keyword value. |
