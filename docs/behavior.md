# Behavior

The behaviors below specify observable outcomes for the Sandbox object and its execution contract. Each behavior uses the form **Initial State → Operation → Result / Final State**. Error attribution (TrapError, SandboxError, ServiceError) is covered in the Error Scenarios subsection; where an error branch is noted below, refer to that subsection for full semantics.

The Sandbox exposes two synchronous invocation verbs — `#eval` (one-shot mruby source execution, B-02 / B-03 / B-06) and `#run` (entrypoint dispatch into a preloaded constant, B-31) — plus the setup verb `#preload` (snippet registration, B-32 / B-33). The four-outcome guarantee, per-invocation isolation, and two-step attribution decision apply uniformly to both invocation verbs. `Kobako::Pool` (B-46..B-48) hands out exclusively-held warm Sandboxes; a pooled Sandbox satisfies every anchor in this document identically.

The governing summary of this document — including the four-outcome guarantee for every Sandbox invocation and the two-step attribution decision — lives in `SPEC.md` § Behavior; this document is the per-anchor reference.

---

## B-01 — Construct a new Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | No `Kobako::Sandbox` instance exists. No Guest Binary is running. |
| **Operation** | `Kobako::Sandbox.new` — optionally with the following keyword arguments: `timeout:` (Numeric seconds, default `60.0`), `memory_limit:` (Integer bytes, default `1 << 20` = 1 MiB), `stdout_limit:` (Integer bytes, default `1 << 20` = 1 MiB), `stderr_limit:` (Integer bytes, default `1 << 20` = 1 MiB). Each of the four caps accepts `nil` to disable that bound. |
| **Result / Final State** | A Sandbox instance is returned. No invocation entry point runs. The stdout and stderr buffers are empty. The snippet table (B-32) is empty. The Sandbox is ready to accept setup calls (`#define`, `#preload`) and invocations (`#eval`, `#run`). |
| **Notes** | `timeout` is absolute wall-clock time from the invocation entry point (`Sandbox#eval` or `Sandbox#run`); the deadline expires at `entry_time + timeout` and is checked at guest wasm safepoints. No trap fires while host code runs, but the wall-clock time a Service callback consumes counts against the deadline — the Host App is responsible for keeping handler execution bounded. `memory_limit` bounds the per-invocation linear-memory delta: cumulative `memory.grow` past the linear-memory size observed at invocation entry, so the Guest Binary's initial allocation and prior invocations' watermark sit outside the budget (E-20). `stdout_limit` / `stderr_limit` bound per-channel output capture (B-04). Setup calls (B-07 / B-08 / B-32) are permitted at any point before the first invocation; B-33 seals both sets. Construction performs the one-time wasm runtime setup from `wasm_path` plus the ABI version probe (B-40); setup failures raise `Kobako::SetupError` (E-40..E-42) or, for an invalid cap argument, `ArgumentError` (E-39). The module compile is amortised across processes by a best-effort disk cache at `$XDG_CACHE_HOME/kobako` (fallback `~/.cache/kobako`, owner-only); any cache failure falls back to in-process compilation with no observable difference beyond construction latency, an entry carries exactly the trust of the Guest Binary file itself, and the directory stays bounded across Guest Binary rebuilds. |

---

## B-02 — Invoke `#eval(code)` from a fresh Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with zero prior invocations (no `#eval` and no `#run` call). Zero or more Members have been bound. Zero or more snippets have been preloaded (B-32). The stdout and stderr buffers are empty. |
| **Operation** | `sandbox.eval(code)` where `code` is a String of mruby source. |
| **Result / Final State** | The Catalog::Handles counter is reset and no Handles from any prior invocation are reachable. Service bindings registered on this Sandbox remain active. Preloaded snippets (B-32) replay in insertion order before `code` executes; each snippet contributes its top-level side effects to the invocation's canonical boot state (B-49). `code` then loads with backtrace filename `(eval)`. `#eval` blocks until execution completes, up to the configured `timeout`. On success, `#eval` returns a single deserialized Ruby value — the last mruby expression of `code`. The stdout and stderr buffers contain any output written during execution, bounded by `stdout_limit` / `stderr_limit` (B-04). Per-invocation cap exhaustion surfaces as `Kobako::TimeoutError` (wall-clock `timeout` exceeded; E-19) or `Kobako::MemoryLimitError` (per-invocation `memory.grow` delta exceeds `memory_limit`; E-20), both subclasses of `Kobako::TrapError`. If `code` is `nil`, not a String, or fails compilation, `#eval` raises `Kobako::SandboxError`. |
| **Notes** | The return value semantics are detailed in B-06. The first invocation (`#eval` or `#run`) seals the snippet table and Service registration (B-33). |

---

## B-03 — Invoke `#eval` or `#run` on a Sandbox that has already invoked

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance that has completed one or more prior invocations (any combination of `#eval` and `#run`). Members bound before the first invocation remain registered. Snippets preloaded before the first invocation remain registered. |
| **Operation** | `sandbox.eval(code)` or `sandbox.run(target, *args, **kwargs)` — any invocation after the first. |
| **Result / Final State** | Each invocation executes in a fully isolated context, independent of all prior invocations. All capability state (Handles issued in prior invocations) is fully discarded before the new invocation begins. All Service bindings and all preloaded snippets remain active across invocations and are visible to the new invocation. `#eval` returns the last expression of its source; `#run` returns the entrypoint's `#call` return value (B-31). The stdout and stderr buffers are cleared at the start of this invocation and contain only output from this invocation; the per-channel truncation predicates (B-04) reset together with the buffers. Per-invocation cap enforcement (B-02 Result) applies identically to every invocation, regardless of verb. |
| **Notes** | Isolation is unconditional — it holds whether the previous invocation succeeded or raised an error, and applies uniformly across `#eval` / `#run` boundaries; stale-Handle presentation is covered by B-18. |

---

## B-04 — Read `#stdout` / `#stderr` after an invocation returns

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which `#eval` or `#run` has been called and has returned (either with a value or by raising an error). |
| **Operation** | `sandbox.stdout`, `sandbox.stderr`, `sandbox.stdout_truncated?`, or `sandbox.stderr_truncated?` — any combination, any order, any number of times. |
| **Result / Final State** | Each byte reader returns the content (as a UTF-8 String) the guest wrote to its respective output channel during the most recent invocation, up to the configured `stdout_limit` / `stderr_limit`. The buffers do not change between successive reads. The content contains no kobako protocol bytes and no truncation sentinels. When a channel's cap was reached, the host buffer ends at the cap boundary and subsequent guest writes on that channel fail or are dropped — the guest may rescue the failure or ignore it, but no further bytes reach the buffer; this does not cause the invocation to raise an error. Each truncation predicate returns `true` iff its channel hit its cap during the most recent invocation, otherwise `false`. |
| **Notes** | Per-channel caps are set at construction (B-01). The buffers and predicates remain readable after an error-raising invocation and reset at the start of the next one (B-03). |

---

## B-05 — Read `#stdout` / `#stderr` before any invocation

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which neither `#eval` nor `#run` has ever been called. |
| **Operation** | `sandbox.stdout` or `sandbox.stderr`. |
| **Result / Final State** | Each reader returns an empty String (`""`). No error is raised. |

---

## B-06 — Return value semantics of `#eval`

This behavior refines the Result of B-02 / B-03 by specifying the exact value `#eval` produces. The return value semantics of `#run` are specified in B-31.

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance, either fresh (per B-02) or post-invocation (per B-03), with zero or more Members bound and zero or more snippets preloaded. |
| **Operation** | `sandbox.eval(code)` — same invocation as B-02 / B-03. |
| **Result / Final State** | When the guest completes without raising `Kobako::TrapError`, `#eval` returns the deserialized Ruby value of the last mruby expression of `code`. If the last expression evaluates to `nil` (including a `code` with no explicit return expression), `#eval` returns Ruby `nil`. If the last expression is, or contains, a Capability Handle the guest received earlier in this invocation, that Handle is restored to its original host object per B-37. If the last expression produces an object that has no wire representation and is not a Capability Handle, `#eval` raises `Kobako::SandboxError`. |
| **Notes** | Exactly one value is returned per `#eval` call; there is no mechanism to return multiple values or stream values. |

---

## B-07 — Declare a Namespace on a Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which no invocation (`#eval` or `#run`) has yet been called. No Namespace named `Name` exists on this Sandbox. |
| **Operation** | `sandbox.define(:Name)` where `:Name` is a Symbol matching `/\A[A-Z]\w*\z/` (Ruby constant-name form). |
| **Result / Final State** | A `Kobako::Namespace` instance is created and associated with this Sandbox under the name `Name`. The namespace has no members yet. The method returns the new `Kobako::Namespace` instance. The Sandbox's `Catalog::Namespaces` now tracks one additional namespace entry. |
| **Notes** | Declarations are design-time operations sealed by the first invocation (B-33): a non-conforming name raises `ArgumentError` (E-16), and `define` after the seal raises `ArgumentError` (E-18) while the Sandbox remains usable with the registrations that existed at sealing. A namespace may have zero members at declaration time; members are added via B-08. |

---

## B-08 — Bind a Member to a declared Namespace

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Namespace` instance (returned by `sandbox.define`) with no member bound under the name `MemberName`. The owning Sandbox has not yet run its first invocation (B-33). |
| **Operation** | `namespace.bind(:MemberName, object)` where `:MemberName` matches `/\A[A-Z]\w*\z/` and `object` is any Ruby object (class, instance, or module) that responds to the methods guest code will invoke. |
| **Result / Final State** | `object` is registered as the Member named `MemberName` within the namespace. Guest code can now reach this object via the two-level path `<Namespace>::<Member>`. The method returns the `Kobako::Namespace` instance (`self`) to allow chaining. |
| **Notes** | The bound object must remain valid for the lifetime of the Sandbox; the Host App manages its lifecycle. A non-conforming `MemberName` raises `ArgumentError` (E-17). Binding is sealed by the first invocation alongside declaration and preload (B-33): after the seal `bind` raises `ArgumentError` (E-45) and every subsequent invocation's Frame 1 preamble carries exactly the bindings that existed at sealing. |

---

## B-09 — Declare multiple Namespaces on the same Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with one or more Namespaces already declared. |
| **Operation** | `sandbox.define(:OtherName)` with a name distinct from all already-declared namespaces on this Sandbox. |
| **Result / Final State** | A new, independent `Kobako::Namespace` is created alongside the existing namespaces. Each namespace's members are accessible to guest code only via that namespace's own path (e.g., `NamespaceA::Member` and `NamespaceB::Member` are distinct paths with no cross-namespace visibility). Namespaces on different Sandbox instances are fully isolated from each other. |
| **Notes** | There is no declared upper limit on the number of namespaces per Sandbox. Each namespace name within a Sandbox must be unique (duplicate-declare behavior is specified in B-10). |

---

## B-10 — Re-declare a Namespace that already exists (idempotent define)

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with a Namespace already declared under the name `Name`. |
| **Operation** | `sandbox.define(:Name)` — same name as an existing namespace. |
| **Result / Final State** | No new namespace is created. `sandbox.define(:Name)` returns the identical `Kobako::Namespace` object previously created — the same object identity (Ruby `equal?`), not a new instance wrapping the same `Catalog::Namespaces` entry. All previously bound members remain in place. The Sandbox's `Catalog::Namespaces` is not modified. |
| **Notes** | Idempotent re-declaration allows Host Apps to retrieve an existing namespace handle without tracking it externally (e.g., in configuration code spread across multiple files). |

---

## B-11 — Attempt to bind a Member name that is already bound in the same Namespace

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Namespace` instance with a member already bound under the name `MemberName`. |
| **Operation** | `namespace.bind(:MemberName, new_object)` — same member name as an already-bound member. |
| **Result / Final State** | `ArgumentError` is raised. The existing binding is not overwritten. The namespace's member registry is unchanged. |

---

## B-12 — Guest-initiated Transport dispatch to a bound Ruby object

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. A Member is bound at `<Namespace>::<Member>` (e.g., `MyService::KV`). The guest holds a reference to the constant `<Namespace>::<Member>` and calls a method on it. |
| **Operation** | Guest code executes `<Namespace>::<Member>.method_name(arg1, arg2, key: value)` — a synchronous method call from within the mruby script. |
| **Result / Final State** | The Host Gem resolves the target to the Ruby object bound at `<Namespace>::<Member>` and invokes `object.public_send(:method_name, arg1, arg2, key: value)`. The Ruby return value is serialized and returned to the guest as the synchronous result of the call — from the guest's perspective, the call completes as an ordinary synchronous Ruby method invocation. |
| **Notes** | Each dispatch invokes the bound object's method exactly once. Keyword argument names travel on the wire as Symbols (→ [`docs/wire-codec.md`](wire-codec.md) § Type Mapping); the host passes them to `public_send` without further conversion. An unresolved target path surfaces per E-12; a method name that resolves to Ruby's ambient reflection / eval surface is rejected before dispatch (B-42). |

---

## B-13 — Service method returns a primitive value

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated Transport dispatch (B-12) has been dispatched. The bound Ruby object's method returns a value that is **wire-representable**: `nil`, Boolean, Integer, Float, String, binary String, Symbol, Array, or Hash. |
| **Operation** | The Host Gem's wire codec serializes the return value and delivers it to the guest as the dispatch response. |
| **Result / Final State** | The guest receives the return value as the synchronous result of the method call, deserialized to the corresponding mruby type. The value is indistinguishable from a locally computed mruby value. No entry is added to the Catalog::Handles. |
| **Notes** | A value is **wire-representable** if its type is one of `nil`, Boolean, Integer, Float, String, binary String, Symbol, Array of wire-representable values, or Hash with wire-representable keys and values; or it is another `Kobako::Handle`. Collections whose elements are all wire-representable are transmitted in full by value; a return value that is not wire-representable takes the Handle allocation path (B-14). |

---

## B-14 — Service method returns a stateful object (Host-side Handle allocation)

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated Transport dispatch (B-12) has been dispatched. The bound Ruby object's method returns a Ruby object that is not wire-representable — for example, a session object, a connection, or any stateful host resource. |
| **Operation** | A return value is routed through the Handle allocation path if and only if its type is not wire-representable per the definition in B-13. The wire layer then automatically registers the object in the Sandbox's Catalog::Handles. |
| **Result / Final State** | The host-side object is stored in `Catalog::Handles` under a new opaque u32 Handle ID. The guest receives a Capability Handle (an opaque integer token) as the dispatch response value, not the object itself. The guest can pass this Handle as the `target` in subsequent dispatch requests to invoke methods on the same host-side object. The Host App has no API to create or inspect Handles directly — Handle allocation is an internal wire-layer operation. |
| **Notes** | Handle lifecycle (per-invocation scope, ID limits) is specified in B-15..B-21. The host→guest symmetric direction — `#run` arguments containing non-wire-representable objects — is governed by B-34, which routes through the same `Catalog::Handles` allocator and lifecycle rules. |

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
| **Operation** | Guest code invokes a method on a Member and passes the `Kobako::Handle` as one of the arguments (e.g., `Store.put(handle, value)`). |
| **Result / Final State** | The Host Gem deserializes the Handle from the wire representation, looks up its ID in the Catalog::Handles, and passes the resolved Ruby object as the corresponding argument to the host Service method. The Service method receives the actual Ruby object, not an ID or token. The method executes and its return value follows the normal primitive (B-13) or stateful (B-14) path. |
| **Notes** | The guest holds a `Kobako::Handle` mruby proxy, never the raw integer ID. An ID with no live entry surfaces per E-13. |

---

## B-17 — Chained composition: Handle returned by Service A used as target in a subsequent call to Service B

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress. Service A has been called via Transport dispatch and returned a stateful object; the guest holds `handle_a` (a `Kobako::Handle` proxy). |
| **Operation** | Guest code calls a method directly on `handle_a` (e.g., `handle_a.find(id)`), using the Handle as the dispatch target. The wire layer encodes `handle_a` as the `target` field of the Request. |
| **Result / Final State** | The Host Gem resolves `handle_a`'s ID against `Catalog::Handles` and invokes `public_send(:find, id)` on the host-side Ruby object that `handle_a` represents. If that call returns another stateful object, a new Handle `handle_b` is allocated and returned to the guest. Each step in the chain is an independent, synchronous dispatch; there is no implicit multi-hop traversal within a single wire call. |
| **Notes** | Chain depth is unbounded within a single invocation as long as each step produces a Handle that survives to the next call. Every host object reachable by the guest must have entered through an explicit allocation path — either Service-returned (B-14) or `#run` argument auto-wrapped (B-34); there is no implicit intermediate binding path. |

---

## B-18 — Handle issued in invocation N is presented as a target in invocation N+1

| Field | Value |
|-------|-------|
| **Initial State** | Invocation N has completed. The guest code attempts to retain a Handle ID from invocation N and presents it as the `target` in a new invocation (N+1, of either verb). At the start of invocation N+1 the Catalog::Handles has been fully reset: all entries from invocation N (both Service-returned via B-14 and host-injected via B-34) are cleared and the counter restarted. |
| **Operation** | Guest code in invocation N+1 calls a method using the stale Handle ID as the dispatch target. |
| **Result / Final State** | The `Catalog::Handles` lookup for that ID returns `:undefined` — the ID does not exist in the fresh table. The stale Handle is invalid; the Host Gem treats this as an unrecognized target. The error path is E-13. Invocation N+1 is not interrupted for other dispatch requests that do not reference stale IDs. |
| **Notes** | This outcome is unconditional: no Handle survives the invocation boundary regardless of how it was allocated (B-14 service return or B-34 host-injected argument), even when invocation N and N+1 execute the same source (or dispatch the same entrypoint) with the same Service bindings. The Catalog::Handles is reset before the Guest Binary is instantiated for invocation N+1. |

---

## B-19 — Sandbox is discarded: all Handles for that Sandbox become invalid

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Sandbox` instance exists with zero or more completed invocations. The Catalog::Handles is owned by this Sandbox instance. |
| **Operation** | The Sandbox instance is garbage-collected or goes out of scope; Ruby reclaims it. |
| **Result / Final State** | The Catalog::Handles and all its entries are destroyed as part of Sandbox teardown. Every Handle that was issued during any invocation on this Sandbox is permanently invalid. No Handle entry is shared with, transferred to, or reachable from any other Sandbox instance. |
| **Notes** | Handles are not reference-counted and there is no explicit `release` API for individual entries. Validity is scoped to the owning Sandbox and the issuing invocation (B-18), uniformly across allocation sources (B-14 / B-34). |

---

## B-20 — Neither guest nor Host App can construct or dereference a Handle from a raw integer

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress (or about to begin via `#run`). Either the guest mruby has access to an arbitrary integer value (e.g., `42` or any computed integer), or the Host App holds an arbitrary integer it intends to present as a Handle. |
| **Operation** | Code on either side of the boundary attempts to use a raw integer as a Handle — for example, by constructing a `Kobako::Handle`-like object from an integer literal, or by any means other than receiving a Handle from a Service dispatch response (B-14) or from a `#run` host-side auto-wrap (B-34). |
| **Result / Final State** | No valid `Kobako::Handle` object is produced from a bare integer on either side. Neither the guest mruby API nor the Host App API exposes a public constructor that converts an integer to a Handle. A raw integer presented as a dispatch target does not carry the Handle wire encoding (`ext 0x01`); the host-side wire decoder rejects the malformed encoding before the value reaches `Catalog::Handles`. A `Kobako::Handle` instance fabricated on the host side through any non-public path and passed to `#run` raises `ArgumentError` at host pre-flight (E-29). |
| **Notes** | The `Kobako::Handle` class holds the u32 ID internally but does not expose it as a readable integer attribute. Each side has its own enforcement point: host pre-flight rejection (E-29) and blocked guest construction (B-39). Handle ID unguessability is not a security property: the capability boundary rests on `Catalog::Handles` membership plus per-invocation isolation (B-18), not on ID secrecy — a guessed or forged ID grants no reference the invocation was not already handed (B-14 / B-34). |

---

## B-21 — Catalog::Handles exhaustion: allocation attempt beyond the ID cap

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress. The Catalog::Handles counter has reached `0x7fff_ffff` (2³¹ − 1), the maximum valid Handle ID. |
| **Operation** | The Host Gem's wire layer attempts to allocate one additional Handle for a new stateful return value. |
| **Result / Final State** | The allocation fails immediately with a `Kobako::HandlerExhaustedError` (a `Kobako::SandboxError` subclass). The counter is not incremented, no new entry is written to the Catalog::Handles, and no ID is silently truncated or wrapped. The error is raised to the Host App; the current invocation terminates. |
| **Notes** | The fail-fast guard makes the violation visible rather than allowing silent semantic corruption. |

---

## B-22 — Distinct Sandboxes on distinct Threads execute independently

| Field | Value |
|-------|-------|
| **Initial State** | Two or more Ruby Threads exist within the same process. The Host App has constructed one `Kobako::Sandbox` per Thread (honoring the input assumption in Scope → Interaction). |
| **Operation** | Each Thread invokes `#eval` or `#run` only on its own owning Sandbox; no Sandbox is shared across Threads. |
| **Result / Final State** | Each invocation executes independently — capability state, Handle IDs, and capture buffers are scoped per Sandbox and never observed by another Thread's invocation. The wasmtime Engine and the compiled Module for `data/kobako.wasm` are shared at process scope: the first Sandbox in the process pays the Engine init and Module compile cost; subsequent Sandboxes in any Thread amortize against that shared state. |
| **Notes** | Aggregate throughput across Threads is bounded by Ruby's GVL — Kobako's native extension does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is serialized. Ruby-side setup (preamble pack, buffer init) can overlap across Threads, giving modest but non-linear scaling under contention. The Host App is responsible for the one-Thread-per-Sandbox invariant; Kobako provides no locking and concurrent invocations on the same Sandbox are unsupported (Scope → Interaction). |

---

## B-23 — Guest call passes a block to a Service method

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox is executing a mruby script. A Member is bound at `<Namespace>::<Member>`. |
| **Operation** | Guest code executes `<Namespace>::<Member>.method_name(arg1, ...) { |x| ... }` — a method call accompanied by a block. |
| **Result / Final State** | The Host Gem dispatches the call as in B-12, but additionally passes a Yielder into the resolved Service method as its block argument. The Service method observes it as a Ruby Proc: `block_given?` returns `true`, `yield` invokes the Yielder, and it is also accessible as `&block` if the method declares one. The Yielder is valid for the duration of this dispatch only. |
| **Notes** | The block itself is not transmitted as a wire value; only a single bit (`block_given`) on the Request tells the host that a block exists. The block body remains inside the guest and is invoked through B-24's yield round-trip. The Yielder has loose Proc-style arity (extras dropped, missing args filled with `nil`); strict-arity behavior must come from a guest-side lambda, which mruby enforces during B-24. |

---

## B-24 — Service method yields to the guest-supplied block

| Field | Value |
|-------|-------|
| **Initial State** | A Service method, invoked from a guest call that supplied a block (B-23), executes on the host. `block_given?` is `true`. |
| **Operation** | The Service method invokes the Yielder via `yield val` or `block.call(val)` — once or many times. |
| **Result / Final State** | Each invocation is a synchronous round-trip into the guest: the guest executes the block body with the supplied arguments, and the block's last expression value is returned to the Service method as the value of the `yield` expression. The Service method continues executing after each yield until it returns, raises, or is terminated by a `break` from the block (B-25). |
| **Notes** | The round-trip uses the same wasmtime synchronous re-entry model as B-12 dispatches in the other direction. The wall-clock `timeout` and `memory_limit` (B-01) apply to the combined host + guest execution; time spent inside the block counts against the deadline. An exception raised inside the block body that the Service method does not rescue propagates back to the dispatch boundary and reaches the guest as a Service-layer fault (E-11). |

---

## B-25 — Guest block uses `break val` to terminate the yielding Service method

| Field | Value |
|-------|-------|
| **Initial State** | A Service method is mid-execution after `yield val` (B-24). |
| **Operation** | The guest block executes `break val` (where the block is a non-lambda, non-orphan block — the standard form). |
| **Result / Final State** | The Service method's invocation terminates immediately as if it had `return`ed `val`. No code in the Service method body after the `yield` statement runs. The Member call in the guest code (`<Namespace>::<Member>.method_name(...) { ... }`) returns `val`. A Capability Handle in `val` is not restored — the break value returns to the guest, not to host code, so it rides back as the same Handle (B-37 Notes). Subsequent guest code runs normally; `break` does not terminate the enclosing guest method or invocation. |
| **Notes** | This matches standard Ruby `break` semantics — `break` unwinds the most recent yielder. `break` from a deeply-nested block (multiple `Service.outer { Service.inner { break } }`) still terminates only the innermost Service method (B-28). The Service method has no opportunity to observe the break — it is unwound transparently. |

---

## B-26 — Guest block falls through or uses `next val`

| Field | Value |
|-------|-------|
| **Initial State** | A Service method is mid-execution after `yield val` (B-24). |
| **Operation** | The guest block reaches its final expression OR executes `next val` explicitly. |
| **Result / Final State** | `yield` in the Service method returns the block's value (`val` for `next val`, the last expression's value for fallthrough). When that value is, or contains, a Capability Handle, it is restored to its original host object first (B-37). The Service method continues executing the statement after `yield`. |
| **Notes** | `next val` and falling off the end of the block are indistinguishable from the Service method's perspective — both are a normal yield return. |

---

## B-27 — Guest block is a lambda using `break`

| Field | Value |
|-------|-------|
| **Initial State** | A Service method is mid-execution after `yield val` (B-24). The block supplied by the guest is a lambda (e.g., created via `->`, `lambda { }`, or `&:symbol`). |
| **Operation** | The lambda body executes `break val`. |
| **Result / Final State** | The lambda returns `val` to the Service method's `yield` site as the yield value. The Service method continues normally; `break` does **not** terminate the Service method when the block is a lambda. |
| **Notes** | mruby and MRI both treat lambda `break` as a silent normal return — equivalent to `next val` (B-26); a Service method cannot distinguish a lambda block that used `break` from one that fell through with the same final value. |

---

## B-28 — Nested dispatch frames each receive their own block

| Field | Value |
|-------|-------|
| **Initial State** | A guest block (from a Service call `Outer.run { |a| ... }`) is mid-execution, and inside its body it calls another Service with its own block (`Inner.run { |b| ... }`). |
| **Operation** | The inner Service method yields, the inner block runs, then the outer block continues. |
| **Result / Final State** | The two Yielders are independent. The inner Service method yields to the inner block; the outer block remains untouched. A `break` from the inner block terminates only `Inner.run` (B-25); the outer block's execution resumes normally. Nesting depth is bounded only by the wasm stack budget. |
| **Notes** | Each guest dispatch frame holds at most one block reference; nested frames stack in LIFO order, matching the synchronous re-entry call structure. The Host Gem does not assign opaque identifiers to blocks — the dispatch frame itself identifies which block any given `yield` targets. |

---

## B-29 — Service method yields multiple times before returning

| Field | Value |
|-------|-------|
| **Initial State** | A Service method has been invoked with a block (B-23). |
| **Operation** | The Service method body executes `yield` multiple times (e.g., looping over a host-side collection: `items.each { |x| yield x }`). |
| **Result / Final State** | Each `yield` is an independent synchronous round-trip into the same guest block. The block body is executed once per yield with the supplied arguments. A `break` (B-25) at any iteration terminates the Service method immediately; otherwise the Service method continues to subsequent iterations. The Service method's return value (when not broken out of) is its own last expression, not the block's final value. |
| **Notes** | The block is reusable within the dispatch — there is no per-yield setup or teardown beyond the round-trip itself. |

---

## B-30 — Service method receives a block but never yields

| Field | Value |
|-------|-------|
| **Initial State** | A Service method has been invoked with a block (B-23). |
| **Operation** | The Service method body completes without ever invoking `yield` or `block.call`. |
| **Result / Final State** | The block is silently discarded. The Service method's return value flows back to the guest as a normal Response (B-13 or B-14). No yield round-trip occurs; the guest block body is never executed. |
| **Notes** | This matches standard Ruby semantics: passing a block to a method that ignores it has no observable effect beyond the block being constructed. |

---

## B-31 — Invoke `#run(target, *args, **kwargs)` for entrypoint dispatch

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance, either fresh (per B-02) or post-invocation (per B-03). At least one snippet preloaded via B-32 defines a top-level constant named `target` that responds to `#call`. |
| **Operation** | `sandbox.run(target, *args, **kwargs)` where `target` is a Symbol or String matching `/\A[A-Z]\w*\z/`. `args` is zero or more positional arguments; `kwargs` is zero or more Symbol-keyed keyword arguments. Argument elements (positional and keyword values) may be any Ruby value — wire-representable values cross the boundary by value, and non-wire-representable values are auto-wrapped into Capability Handles per B-34. |
| **Result / Final State** | The host normalizes `target` to Symbol via `.to_sym` and applies host pre-flight checks (target type, target pattern, args / kwargs shape, Handle-forgery rejection). Non-wire-representable argument elements are routed through host-side auto-wrap (B-34) and arrive in the guest as `Kobako::Handle` proxies. Preloaded snippets (B-32) replay in insertion order. The guest then resolves the Symbol as a top-level constant on `Object` (no `::` nesting is parsed; the Symbol names a single constant on the top-level scope), confirms it responds to `#call`, and invokes `target.call(*args, opts)` where `opts` is the kwargs Hash, omitted from the argv when empty. `#run` returns the deserialized Ruby value produced by that call; a Capability Handle in that value — bare or nested in an Array / Hash — is restored to its original host object per B-37. Per-invocation cap enforcement, capability state reset, and stdout / stderr buffer behavior follow B-02 / B-03 identically. |
| **Notes** | The entrypoint convention is duck-typed on `#call`: any constant whose value is a `Proc`, `Class`, `Module`, or instance responding to `#call` is acceptable. Entrypoints accept the kwargs Hash as a trailing positional parameter (`def call(req, opts = {})`, `->(req, opts) { ... }`); positional-only signatures are valid for kwargs-free invocations. The first `#run` (or first `#eval`, B-02) seals the snippet table and Service registration (B-33). Error scenarios are tabled in § Entrypoint dispatch errors (E-24..E-31, plus the reused anchors listed there). The `#run` backtrace contains no `(eval)` frame because no user source was loaded for the invocation; the trailing frame is `(snippet:Name)` (B-32). |

---

## B-32 — Preload a snippet onto a Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which no invocation (`#eval` or `#run`) has yet been called. No snippet is currently registered under the canonical name resolved by this call. |
| **Operation** | One of two forms: (a) `sandbox.preload(code: source, name: Name)` where `source` is a String of mruby source and `Name` is a Symbol or String matching `/\A[A-Z]\w*\z/`; or (b) `sandbox.preload(binary: bytecode)` where `bytecode` is a String of RITE bytecode bytes. The `binary:` form does not accept a `name:` keyword — the snippet name, when present, comes from the bytecode's embedded `debug_info` filename. |
| **Result / Final State** | The snippet is appended to the Sandbox's insertion-ordered snippet table. The `code:` form validates `Name` against the constant pattern and trial-loads the source at preload time — the trial load's side effects reach no invocation; compile errors raise `Kobako::SandboxError` (E-32) and the snippet table is not modified. The `binary:` form records the bytecode bytes verbatim into the snippet table; RITE version mismatch (E-37) and corrupt body (E-38) are detected by the guest during the first invocation's snippet replay and raise `Kobako::BytecodeError`. Bytecode compiled without a `debug_info` section is a legal `binary:` payload — the guest loads it normally and the snippet contributes its top-level effects without a backtrace filename. On successful preload, the method returns the Sandbox instance (`self`) to allow chaining. From this point on, every subsequent invocation (`#eval` or `#run`) replays the snippet — in insertion order, before any per-invocation source or entrypoint resolution — into the invocation's canonical boot state (B-49). |
| **Notes** | The canonical name is the snippet's diagnostic identity: when present, it is the filename in the loaded IREP's `debug_info` and appears in every backtrace frame from the snippet as `(snippet:Name):line`. The `code:` form sets the compile ccontext filename to `Name`, so the name is always present; the `binary:` form carries whatever filename the producing tool baked in, and frames from a snippet without `debug_info` (e.g., `mrbc` without `-g`) are omitted from `Exception#backtrace` per upstream mruby semantics while exception class, message, and `origin` attribution remain intact. Error scenarios are tabled in § Preload errors (E-32..E-38). Inter-snippet dependencies (e.g., snippet B referencing a constant defined by snippet A) require A to be preloaded before B; insertion order is the contract. |

---

## B-33 — Snippet table sealing on first invocation

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with zero or more snippets preloaded via B-32 and zero or more Namespaces / Members registered via B-07 / B-08. No invocation has yet been called. |
| **Operation** | The first invocation — `sandbox.eval(...)` or `sandbox.run(...)` — is called. |
| **Result / Final State** | The snippet table becomes immutable. The snippet set replayed on every subsequent invocation is exactly the set registered at the moment of sealing, in insertion order. Any further call to `sandbox.preload(...)` raises `ArgumentError` (E-35); the existing snippet table is preserved unchanged. Service registration (B-07 / B-08) is sealed simultaneously by the same first invocation. |
| **Notes** | After the seal, `#define` raises `ArgumentError` (E-18), `namespace.bind` raises `ArgumentError` (E-45), and `#preload` raises `ArgumentError` (E-35). The two registries are stored and validated independently; the sealing boundary is the only event they share. |

---

## B-34 — `#run` argument auto-wraps into a host-side Handle when not wire-representable

| Field | Value |
|-------|-------|
| **Initial State** | `#run(target, *args, **kwargs)` is invoked (B-31). At least one element of `args`, or at least one value in `kwargs`, is not wire-representable per the type set defined in B-13 — for example, a `StringIO`, an arbitrary Host App `Env` instance, or any other Ruby object whose class is outside the wire 12-entry mapping. |
| **Operation** | During Invocation envelope encoding the Host Gem walks the `args` Array and the `kwargs` Hash values; container types (Array, Hash) are walked one level at a time, and each leaf value that is not wire-representable is allocated into the Sandbox's Catalog::Handles. The allocator returns a fresh u32 ID (B-15) which is written into the envelope as an `ext 0x01` Capability Handle in place of the original Ruby object. Wire-representable leaves pass through unchanged. |
| **Result / Final State** | The guest mruby code receives a `Kobako::Handle` proxy at each position where the host supplied a non-wire-representable argument. The proxy carries no observable Ruby value content; method calls on it dispatch back to the host through the same `method_missing` → Transport path the guest uses for Service-returned Handles (B-17). The host-side `Catalog::Handles` entry remains live for the duration of the invocation and is cleared together with all other Handles at the invocation boundary (B-18 / B-19). `Catalog::Handles` cap exhaustion during the walk raises `Kobako::HandlerExhaustedError` at host pre-call via the same path as B-21 / E-07. |
| **Notes** | This behavior is symmetric with B-14 (Service-returned stateful objects): both directions of the boundary route non-wire-representable Ruby objects through the Catalog::Handles allocator under identical lifecycle rules. The walk traverses Array and Hash containers but does not descend into instance variables or other internal structure of non-wire-representable leaves — once a leaf is identified as needing wrapping, its sub-structure is hidden behind the Handle. A `Kobako::Handle` value already produced internally by the Host Gem (e.g., an instance fabricated through any non-public path) is rejected at host pre-flight (E-29); auto-wrap never re-wraps an existing Handle. |

---

## B-35 — Read `#usage` for per-last-invocation resource accounting

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance, either fresh (no prior invocation) or post-invocation. |
| **Operation** | `sandbox.usage` — any number of times, any order, before or after `#eval` / `#run`. |
| **Result / Final State** | Returns a `Kobako::Usage` value object exposing two readers: `wall_time` (Float seconds the guest export call spent inside the wasmtime engine during the most recent invocation) and `memory_peak` (Integer bytes, the high-water mark of the per-invocation `memory.grow` delta past the linear-memory size observed at invocation entry). Both readers reflect the most recent invocation only — the next `#eval` / `#run` overwrites them at the start of that invocation. Before any invocation, `#usage` returns the pre-invocation sentinel `Kobako::Usage::EMPTY` (`wall_time` = `0.0`, `memory_peak` = `0`). |
| **Notes** | `wall_time` is measured on the host around the guest export call (`__kobako_eval` / `__kobako_run`): the bracket opens when the per-invocation caps are armed and closes when wasmtime returns control to the host, so it includes time spent in host Service callbacks (consistent with the `timeout` accounting in B-01 Notes) and excludes the post-export `OUTCOME_BUFFER` fetch, its msgpack decode, and the stdout / stderr capture readout. `memory_peak` shares its baseline accounting with `memory_limit` (B-01, E-20). Both readers are populated on every invocation outcome — value return or any raised error class — so the Host App can read `#usage` after a rescue; on `MemoryLimitError`, `memory_peak` reports the largest delta the limiter accepted, never exceeding `memory_limit`. |

---

## B-36 — Guest probes a Member or Handle with `respond_to?`

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The guest holds a Member constant `<Namespace>::<Member>` (B-08) or a `Kobako::Handle` instance obtained from a prior dispatch (B-14) or `#run` auto-wrap (B-34). |
| **Operation** | Guest code calls `<Namespace>::<Member>.respond_to?(:any_name)` on the Member constant, or `handle.respond_to?(:any_name)` on the Handle instance, for a name the proxy does not define locally. |
| **Result / Final State** | `respond_to?` returns `true` for every such probe, on both the Member constant and the Handle instance. The probe is answered entirely inside the guest — no Transport Request is sent. A following method call dispatches normally (B-12 for a Member, B-17 for a Handle). |
| **Notes** | Every method call on a Member or Handle is forwarded to the host, so `respond_to?` answers `true` to stay consistent; the answer is optimistic, not authoritative — it does not consult the host and does not confirm the bound object implements the method. An unimplemented method surfaces at dispatch as `type="runtime"` (E-11), distinct from the unresolvable-target `type="undefined"` (E-12 / E-13). Names the proxy defines locally resolve through their own methods and never reach this path. |

---

## B-37 — Guest returns a Capability Handle across the boundary (Host-side Handle restoration)

This behavior refines the value-return semantics of B-06 (`#eval`) and B-31 (`#run`), and of the yield-block ok result (B-26), for the case where the returned value is, or contains, a Capability Handle.

| Field | Value |
|-------|-------|
| **Initial State** | The guest hands a value back to host code — either as the invocation result (the last mruby expression of `#eval`, or the entrypoint's `#call` return for `#run`), or as the value a guest block returns to a yielding Service method's `yield` expression (the YieldResponse `0x01` ok payload of B-26). That value is a `Kobako::Handle` proxy the guest received earlier in this same invocation (from a Service return per B-14, or from a `#run` argument auto-wrap per B-34), or an Array / Hash containing one or more such proxies. |
| **Operation** | The guest serializes the value through the same wire codec used for every guest→host value; each `Kobako::Handle` proxy encodes as an `ext 0x01` Capability Handle carrying its Handle ID, which both the Result envelope `value` position and the YieldResponse ok payload position accept. On the host, after decoding the payload, the Host Gem walks the decoded value — descending into Array elements and Hash keys and values one level at a time — and replaces each decoded Capability Handle with the host-side object bound to its ID in this invocation's Catalog::Handles. Wire-representable leaves pass through unchanged. |
| **Result / Final State** | At each position where the guest returned a Handle, the host yields the original host-side object instance — the same object Catalog::Handles holds. For the invocation result, `#eval` / `#run` returns that object (bare, or inside a restored Array / Hash with all other structure preserved). For a yield-block ok result, the Service method's `yield` expression (B-26) receives that object. The Host App and Service code receive ordinary host objects and never observe a `Kobako::Handle`. Restoration is a read against Catalog::Handles, not an allocation: no new Handle ID is issued and the table is unchanged. |
| **Notes** | B-37 is the symmetric inverse of B-34, operating on the same `Catalog::Handles` table under the same per-invocation lifecycle (B-15, B-19); a `0x02` break value is excluded — it returns to the guest rather than to host code, riding back unrestored on the same ID (B-25) — and the `0x04` error YieldResponse carries no value to restore (E-22). Because the guest cannot fabricate a Handle (B-20), every Handle the guest can legitimately return resolves to a live object; a returned ID with no live binding indicates a corrupted runtime and raises `Kobako::SandboxError` through the same wire-violation fallback as a malformed Result value (E-09). |

---

## B-38 — Guest attempts to construct a Member proxy

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The guest holds a Member constant `<Namespace>::<Member>` (B-08) whose Host App binding is an object the guest reaches by calling methods on the constant (B-12). |
| **Operation** | Guest code calls `<Namespace>::<Member>.new(...)` or `<Namespace>::<Member>.allocate` — any attempt to instantiate the Member constant. |
| **Result / Final State** | The call raises `NoMethodError` inside the guest. No instance is produced and no Transport Request is sent — the construction attempt never reaches the host. When the guest does not rescue it, the exception reaches the invocation top level and is attributed as `Kobako::SandboxError` per E-04, identical to any other uncaught guest exception. |
| **Notes** | A Member is a dispatch target, not a constructible type; blocking `new` and `allocate` — the two construction entries mruby exposes — keeps the proxy surface to dispatch only and fails fast. The proxy defines both locally as raising methods, so B-36's optimistic `respond_to?` answer does not apply to them. B-39 blocks the `Kobako::Handle` proxy through the same entries; the two proxies differ only in that a Handle is still constructed internally by the wire decoder (B-14 / B-34) through a path that bypasses the blocked Ruby entries. |

---

## B-39 — Guest attempts to construct a Handle proxy

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The `Kobako::Handle` proxy class is reachable as a named constant, but the guest has not received a Handle from a Service return (B-14) or a `#run` argument auto-wrap (B-34). |
| **Operation** | Guest code calls `Kobako::Handle.new(...)` or `Kobako::Handle.allocate` — any attempt to fabricate a Handle proxy, with or without an integer ID argument. |
| **Result / Final State** | The call raises `NoMethodError` inside the guest. No proxy is produced and no Handle ID is bound. When the guest does not rescue it, the exception reaches the invocation top level and is attributed as `Kobako::SandboxError` per E-04, identical to any other uncaught guest exception. |
| **Notes** | This is the guest-side enforcement of B-20; the Host App side is enforced separately at host pre-flight (E-29). Both entries raise regardless of passed arguments rather than tripping an arity check first. The wire decoder's restoration path (B-14 / B-34) constructs a Handle through an internal instance-allocation path that does not dispatch the blocked Ruby entries. Parallels B-38. |

---

## B-40 — Host validates the Guest Binary ABI version at construction

| Field | Value |
|-------|-------|
| **Initial State** | No `Kobako::Sandbox` instance exists. A Guest Binary artifact is present at the resolved `wasm_path` and exports `__kobako_abi_version` returning the ABI version the Host Gem implements (→ [`docs/wire-codec.md`](wire-codec.md) § ABI Version). |
| **Operation** | `Kobako::Sandbox.new` — with the default bundled Guest Binary or a custom `wasm_path:`. |
| **Result / Final State** | Construction probes `__kobako_abi_version` after the wasm runtime setup of B-01 and compares the reported value against the Host Gem's implemented version by equality. On equality, construction completes per B-01. No invocation entry point runs. |
| **Notes** | The probe is the only guest function construction calls — `__kobako_abi_version` is a pure constant function (→ [`docs/wire-codec.md`](wire-codec.md) § ABI Version), so no invocation state exists before or after it. The check exists for Guest Binaries that ship independently of the Host Gem: the bundled `data/kobako.wasm` matches by construction, while a custom guest built against a different ABI version fails loudly at `Sandbox.new` (E-42) instead of misbehaving mid-invocation. An absent export is the same failure — a guest predating the version export is by definition built against a different ABI. |

---

## B-41 — Guest regexp matching as a compute capability

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The Guest Binary provides `Regexp` and `MatchData` as guest-visible Ruby classes — a regexp literal (`/.../`), `Regexp.new` / `Regexp.compile`, and the `String` integration methods — that compile and run patterns against `String` values. Neither class is among the 12 wire types (→ [`docs/wire-codec.md`](wire-codec.md) § Type Mapping). |
| **Operation** | Guest code compiles a pattern and matches it against a `String`, then uses the result inside the invocation — a `MatchData`, an Integer match index, `nil` for no match, captured substrings (positional or by name), or the refreshed match backref globals. |
| **Result / Final State** | Matching runs entirely inside the Guest Binary; `Regexp` and `MatchData` are guest-internal and never cross the wire. A value the guest hands back to host code reduces to a wire type first — a captured substring (`str`), a match index (`int`), a capture list (`array`), a `named_captures` map (`map`), absent-match `nil`, or a `Symbol`; a bare `Regexp` or `MatchData` in a returned position is a non-wire value governed by the ordinary return-value semantics (B-06). A pattern that fails to compile raises `RegexpError` inside the guest; uncaught, it is attributed as `Kobako::SandboxError` per E-04. |
| **Notes** | Regexp is a Rust capability gem composed into the Guest Binary shell, the pure-compute peer of the IO / Kernel surface (B-04); like every guest stdlib capability it carries no per-feature wire contract — the closed 12-entry wire type set already excludes `Regexp` / `MatchData`, so projecting a result to wire types is structural, not a new envelope. Coverage is a curated subset of the CRuby `Regexp` / `MatchData` API, byte-based throughout, following MRI within that subset except where a per-behavior contract states otherwise. The full surface and the per-behavior contracts (anchored `RX-xx`) live in [`docs/regexp.md`](regexp.md). |

---

## B-42 — Host rejects ambient reflection / eval methods at guest→host dispatch

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code dispatches to a target resolved through `Catalog::Namespaces` or `Catalog::Handles` (B-12, B-16, B-17). The target is a bound Service object — a plain object, a `Proc` / lambda bound as a Member, or a host object received earlier as a Capability Handle. |
| **Operation** | Guest code invokes a method whose name resolves, on the target, to Ruby's ambient reflection / metaprogramming surface — the `send` family, `eval` / `instance_eval` / `instance_exec` / `class_eval` / `module_eval`, `binding`, `method` / `public_method`, `define_method`, `const_get` / `const_set`, `instance_variable_get` / `instance_variable_set`, or any other method whose resolved owner is a core meta module (`BasicObject`, `Kernel`, `Object`, `Module`, `Class`) or a callable gadget type (`Proc`, `Method`, `UnboundMethod`, `Binding`). |
| **Result / Final State** | The host rejects the call before it reaches the target: the dispatch returns error `type="undefined"` and no method runs on the host. Only methods the bound object itself exposes as Service behaviour are reachable. For a `Proc` or `Method` target the callable allowlist — `call`, `[]`, `yield`, `arity`, `lambda?` — is the sole exception: these invoke or describe the callable and stay reachable, while every other `Proc` / `Method` / `UnboundMethod` / `Binding` method (notably `Proc#binding`, `Method#receiver`, `Method#unbind`, `Binding#eval`) is rejected. Unrescued in the guest, the rejection reaches the Host App as `Kobako::ServiceError` per E-43. |
| **Notes** | This is the host-authoritative enforcement point: the decision rests on the resolved method's owner, so it holds regardless of what the guest sends — a guest that forges a dispatch Request directly is bound identically to one going through the guest proxy (B-44 is the non-authoritative guest-side mirror). The contract is least-privilege: a bound object's reachable surface is its own Service methods plus, for callables, the invocation allowlist. A method name with no concrete public method on the target is allowed only when the target opts in via `respond_to?` (dynamic `method_missing` Services), since the ambient methods above are all concretely defined and never reach that branch. Reflective objects never become reachable Handles in the first place — B-43 keeps `Binding` / `Method` / `UnboundMethod` off the wire entirely. |

---

## B-43 — Reflective gadget objects are not wire-representable

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated Service dispatch (B-12, B-16, B-17) returns a `Binding`, `Method`, or `UnboundMethod` instance. |
| **Operation** | The host wire layer evaluates the return value for wire representation and Capability Handle wrapping (B-13, B-14). |
| **Result / Final State** | The host refuses to mint a Capability Handle for a `Binding`, `Method`, or `UnboundMethod`: these types are neither wire-representable nor Handle-wrappable. The dispatch reports error `type="runtime"` (E-44) instead of returning a Handle, so the guest never receives a callable proxy onto a host reflection object. A gadget nested inside an otherwise non-representable Array / Hash rides back inside that container's Handle and is refused the same way when the guest extracts it (the extraction is itself a B-12 dispatch return). A `Proc` is excluded — it stays Handle-wrappable, and any reflective method on the resulting Handle (e.g. `#binding`) is rejected by B-42. |
| **Notes** | Defense in depth behind B-42, closing the second hop of a `Proc#binding` → `Binding#eval` escalation: even were a reflective method reachable, its gadget result cannot cross back as a Handle. The three blocked types are the ones whose reachable surface is wholly reflection (`Binding#eval`, `Method#receiver` / `#unbind`, `UnboundMethod#bind`); a `Proc`'s legitimate use is invocation (B-42 callable allowlist), so it stays wrappable. The refusal lives at the single Handle mint point (`Catalog::Handles`), so it holds on both wrap paths: the Service-return path reports `type="runtime"` (E-44), and the `#run` host→guest auto-wrap (B-34) refuses a gadget argument as a host-side `Kobako::SandboxError` before the invocation runs, the same surface as the B-21 cap failure during auto-wrap (E-07). |

---

## B-44 — Guest proxy mirrors the reflection rejection

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code holds a Member constant (B-08) or a Capability Handle (B-14); the guest proxy forwards method calls to the host via the Transport (B-12). |
| **Operation** | Guest code invokes a reflection / eval method by name on a Member or Handle proxy — a name in the recognized denylist: the `send` family (`send`, `__send__`, `public_send`), `eval` / `instance_eval` / `instance_exec` / `class_eval` / `module_eval`, `binding`, `method` / `public_method` / `instance_method`, `define_method` / `define_singleton_method`, `const_get` / `const_set`, `instance_variable_get` / `instance_variable_set`, `singleton_class`, and the non-allowlisted callable methods `curry` / `to_proc` / `receiver` / `unbind`. |
| **Result / Final State** | The guest proxy rejects the call before emitting a Transport Request: it raises `NoMethodError` inside the guest and sends no wire Request. Unrescued, the exception reaches the invocation top level and is attributed as `Kobako::SandboxError` per E-04, identical to any other uncaught guest exception. The callable allowlist (B-42) is preserved — `call`, `[]`, `yield`, `arity`, `lambda?` forward normally. |
| **Notes** | This is the guest-side mirror of B-42, parallel to how B-39 mirrors the host's Handle-forgery enforcement (E-29): opacity and fail-fast UX, not the security boundary. B-42 is authoritative and complete — it decides on the resolved method's owner, so a name the guest denylist misses, a guest binary that bypasses this proxy, or a forged Request is still rejected host-side. The guest enforces on method name because it cannot resolve the bound object's method owner; its denylist is therefore a best-effort recognized set, not the exhaustive contract. |

---

## B-45 — Guest ambient time and randomness are denied (deterministic guest execution)

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The Guest Binary links no mrbgem exposing time, sleep, or randomness (the strict allowlist, → [`SPEC.md`](../SPEC.md) Goals), and reaches the host only through injected Services (B-08) and the stdout / stderr write surface (B-04). |
| **Operation** | Guest code — or a Rust capability gem linked into the Guest Binary — reaches for ambient wall-clock time or entropy through the WASI layer (`wasi:clocks/wall-clock`, `wasi:clocks/monotonic-clock`, or `wasi:random`), whether via libc (`time`, `gettimeofday`, `getrandom`) or a Rust `SystemTime` / `Instant` / RNG. |
| **Result / Final State** | The host denies every ambient source: `wasi:clocks` reads the Unix epoch and never advances, and `wasi:random` yields a constant byte stream. Guest code observes no real wall-clock time and no host entropy through any ambient path; the only time or randomness available to it is a value a Service injects (B-12) or a snippet embeds (B-32). Given identical source, snippets, and Service responses, guest execution is reproducible. |
| **Notes** | The denial is a property of the host's WASI context, layered behind the mrbgem allowlist: a future Guest Binary gem that reaches libc time or randomness obtains the frozen, deterministic values rather than ambient ones, so the no-ambient-nondeterminism guarantee does not rest on the allowlist alone. The per-invocation wall-clock `timeout` (B-01) is unaffected — it is measured on the host clock and enforced by the engine, never the guest's frozen `wasi:clocks`. A Host App that needs real time or randomness inside the guest injects it explicitly as a Service value, the same mediation every host capability takes. |

---

## B-46 — Construct a Kobako::Pool

| Field | Value |
|-------|-------|
| **Initial State** | No `Kobako::Pool` instance exists. The Guest Binary is resolvable per B-01. |
| **Operation** | `Kobako::Pool.new(slots: n) { \|sandbox\| ... }` — `slots:` (positive Integer) is the number of pooled Sandboxes; `checkout_timeout:` (Numeric seconds, default `5.0`, `nil` to wait indefinitely) bounds the B-47 checkout wait; every other keyword argument is forwarded verbatim to `Kobako::Sandbox.new`; the optional block is the per-Sandbox setup hook. |
| **Result / Final State** | A Pool managing up to `slots` Sandboxes is returned. No Sandbox is constructed yet: a checkout (B-47) receives an idle constructed Sandbox when one exists, and constructs a new one — with the forwarded keyword arguments — only when no idle Sandbox exists and fewer than `slots` have been constructed. The setup block runs exactly once per pooled Sandbox immediately after its construction, before that Sandbox is first handed to any checkout caller. No invocation entry point runs. |
| **Notes** | The setup block is the pooled Sandbox's setup window (B-07 / B-08 / B-32); its registrations seal at that Sandbox's first invocation (B-33) exactly as on a directly constructed Sandbox. Sandbox construction and setup-block errors surface unchanged — original class and message — at the `#with` call whose checkout triggered the creation (E-39..E-42, or the block's own exception). Invalid `slots:` / `checkout_timeout:` raise `ArgumentError` at `Pool.new` (E-47). Pool construction signals the runtime to provision instance resources for slot reuse; provisioning is not observable — a pooled Sandbox satisfies every behavior anchor identically (B-47). |

---

## B-47 — Check out a Sandbox via Pool#with

| Field | Value |
|-------|-------|
| **Initial State** | A Pool (B-46). Any number of threads call `#with` concurrently. |
| **Operation** | `pool.with { \|sandbox\| ... }` |
| **Result / Final State** | The calling thread holds exclusive use of one pooled Sandbox for the duration of the block; `#with` returns the block's return value. When all `slots` Sandboxes are held, the call blocks until one is returned, or raises `Kobako::PoolTimeoutError` once the wait exceeds `checkout_timeout` (E-46). At block exit — normal return or raised exception — the Sandbox returns to the pool. At checkout, `#stdout` / `#stderr` read as empty and both truncation predicates are false; Service bindings and snippets registered by the setup block remain active. Every B-xx / E-xx behavior holds for a pooled Sandbox exactly as for a directly constructed one — in particular per-invocation isolation (B-03) guarantees that no guest-observable state crosses from one checkout holder to the next. |
| **Notes** | Checkouts are independent: a nested `#with` on the same thread checks out a second Sandbox and counts against `slots` like any other holder. Per-checkout exclusivity is what extends B-22's one-thread-at-a-time contract to pooled Sandboxes. A Sandbox that raised `Kobako::TrapError` during a checkout is discarded at checkin — the pool applies the discard-and-recreate recovery contract itself, refilling the slot by a fresh construction + setup-block run on next demand. |

---

## B-48 — Pool teardown follows Pool reachability

| Field | Value |
|-------|-------|
| **Initial State** | A Pool holding constructed Sandboxes; zero or more are checked out. |
| **Operation** | The Host App drops its last reference to the Pool. |
| **Result / Final State** | The Pool and the pooled Sandboxes it holds become unreachable and are reclaimed by Ruby garbage collection like ordinary objects, releasing every runtime resource they held. A Sandbox held by an in-flight `#with` block remains valid until that block exits. |
| **Notes** | The Pool has no explicit teardown verb; reachability is the lifecycle. This mirrors B-19 — discarding the owning object is the resource-release path. |

---

## B-49 — Every invocation begins from the canonical boot state

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox with any invocation history — none, successful, or failed. |
| **Operation** | Any invocation entry — `sandbox.eval(code)` or `sandbox.run(target, ...)`. |
| **Result / Final State** | The mruby interpreter the invocation observes starts in the canonical boot state: the deterministic post-boot interpreter state of the Guest Binary, identical for every invocation of the same artifact (B-45) and carrying no artifact of any prior invocation (B-03). On top of that state, in order: the Frame 1 preamble installs the Sandbox's registrations, preloaded snippets replay (B-32), and then per-invocation source load (`#eval`) or entrypoint resolution (`#run`) proceeds. |
| **Notes** | The canonical boot state may be computed at build time and embedded in the Guest Binary as a pre-initialized image; embedding is unobservable, and re-baking the same inputs yields a byte-identical artifact — the reproducible-build pipeline (F-10) gates this. The engine may likewise provision per-invocation instance resources for reuse (B-46 Notes); provisioning is equally unobservable. |

---

## Error Scenarios

Every Sandbox invocation (`#eval` or `#run`) terminates in exactly one of four outcomes: a return value, `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError`. Attribution is determined by a two-step decision applied after the invocation export returns (`__kobako_eval` for `#eval`, `__kobako_run` for `#run`):

**Step 1 — Trap detection (highest priority).**
If the Wasm engine reports a trap (e.g., wasmtime raises a native trap exception), the outcome is `Kobako::TrapError` or one of its named subclasses regardless of any other state. No outcome bytes are inspected. The trap kind determines the raised class: wall-clock timeout traps raise `Kobako::TimeoutError` (E-19), linear-memory-cap traps raise `Kobako::MemoryLimitError` (E-20), and all other engine or wire-violation traps raise the base `Kobako::TrapError` (E-01..E-03).

**Step 2 — Outcome envelope tag (non-trap outcomes only).**
If no trap occurred, the Host Gem reads the outcome bytes produced by `__kobako_take_outcome` and dispatches on the first-byte tag:

| First-byte tag | Outcome bytes state | Raised class |
|---------------|---------------------|--------------|
| — | Zero-length (`len == 0`) | `Kobako::TrapError` — wire violation fallback (a *wire violation* is any guest binary output that does not conform to the wire codec; → [`docs/wire-codec.md`](wire-codec.md) § Type Mapping) |
| `0x01` (result) | Decode succeeds | Return value (no error raised) |
| `0x01` (result) | Decode fails (malformed MessagePack or unrepresentable value) | `Kobako::SandboxError` |
| `0x02` (panic) | Decode succeeds + `origin == "service"` | `Kobako::ServiceError` |
| `0x02` (panic) | Decode succeeds + `origin == "sandbox"` or missing | `Kobako::SandboxError` |
| `0x02` (panic) | Decode fails (malformed envelope) | `Kobako::SandboxError` |
| Any other tag | — | `Kobako::TrapError` — wire violation fallback |

`stdout` and `stderr` bytes do not participate in attribution dispatch. They are always available via `Sandbox#stdout` / `Sandbox#stderr` after a rescue, including after error-raising runs.

---

### `Kobako::TrapError` and its subclasses

Raised when the Wasm execution engine crashes, when the wire layer detects a structural violation that signals a corrupted guest execution environment, or when a configured per-invocation cap is exceeded. The base class `Kobako::TrapError` covers engine and wire-violation traps; the named subclasses `Kobako::TimeoutError` and `Kobako::MemoryLimitError` cover the configured-cap cases. After any TrapError (base class or subclass), the Sandbox is considered unrecoverable; Host Apps should discard and recreate it before the next invocation.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-01 | Wasm engine trap: `unreachable` instruction, stack overflow, or import signature mismatch | Wasm engine reports a native trap; Step 1 fires | `Kobako::TrapError` |
| E-02 | Guest exited without writing any outcome bytes (`len == 0`) | Step 2: zero-length outcome bytes; wire violation fallback | `Kobako::TrapError` |
| E-03 | Outcome first byte is an unknown tag (not `0x01` or `0x02`) | Step 2: unrecognized tag; wire violation fallback | `Kobako::TrapError` |
| E-19 | Absolute wall-clock time since invocation entry (`Sandbox#eval` or `Sandbox#run`) reached the configured `timeout` and a guest wasm safepoint was hit thereafter (B-01) | Wasm engine reports a wall-clock interrupt at the first guest wasm safepoint after the absolute deadline; Step 1 fires | `Kobako::TimeoutError` |
| E-20 | Cumulative guest `memory.grow` since invocation entry would push past the configured `memory_limit` (B-01) | Wasm engine reports a memory-cap trap; Step 1 fires | `Kobako::MemoryLimitError` |

**Cross-references:** E-02 and E-03 are the wire-violation fallback paths invoked by any malformed Guest Binary output. B-21 (Handle counter exhaustion) raises `Kobako::HandlerExhaustedError` (a `SandboxError` subclass), not `TrapError`. E-19 fires only at guest wasm safepoints — a Service callback running on the host cannot itself trigger E-19 — but the wall-clock time consumed by host callbacks counts against the `timeout` budget (B-01 Notes).

---

### `Kobako::SandboxError`

Raised when the guest execution environment ran to completion but the overall execution failed due to a protocol fault, a mruby runtime error, or a Host Gem–side wire decode failure. The guest Wasm instance is retired normally; the sandbox infrastructure itself is intact.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-04 | Guest mruby script raises an uncaught exception (e.g., `RuntimeError`, `NoMethodError`) that reaches the top level of the invocation export (`__kobako_eval` or `__kobako_run`) | B-02, B-03 — script execution |
| E-05 | The guest fails to compile the source supplied to `#eval` before any execution begins | B-02 — fresh invocation |
| E-06 | The invocation's return value has no wire representation — the `#eval` last expression or the `#run` entrypoint's `#call` return is a raw mruby `Object` with no MessagePack encoding, or nests beyond the maximum encodable depth (a reference cycle necessarily does; → [`docs/wire-codec.md`](wire-codec.md) § Structural Nesting Depth); outcome tag `0x01` is present but the value field fails to decode | B-06, B-31 — return value semantics |
| E-07 | Handle issuance for the returned object fails because the per-invocation Handle counter has reached `0x7fff_ffff` (2³¹ − 1); raised as the `Kobako::HandlerExhaustedError` subclass | B-21 — Handle counter exhaustion |
| E-08 | Outcome tag is `0x02` (panic) and the panic envelope is malformed or missing required fields | Step 2 attribution table |
| E-09 | Outcome tag is `0x01` (result) and the result envelope is malformed or fails MessagePack parse | Step 2 attribution; B-06 fallback |
| E-10 | Guest presents an invalid wire payload as a dispatch argument (e.g., a raw integer where a Capability Handle ext type `0x01` is required) | B-20 — guest cannot forge Handles |
| E-21 | Guest block uses `return val` while its enclosing method is still on the guest call stack (non-lambda, non-orphan Proc); the unwind crosses the host yield boundary, which is unrepresentable on the wire | B-24 — yield round-trip |
| E-22 | Guest block returns a value that has no MessagePack wire representation per [`docs/wire-codec.md`](wire-codec.md) § Type Mapping, or that nests beyond the maximum encodable depth (a reference cycle necessarily does; § Structural Nesting Depth) | B-24 — yield round-trip |
| E-23 | Host Service method invokes its Yielder after the originating dispatch frame has returned (e.g., the Service stored the block via `&block` and called it from a later dispatch or post-dispatch host code) | B-23 — Yielder scope |

---

### `Kobako::ServiceError`

Raised when the guest execution environment ran to completion, the mruby script itself did not crash, but a Service capability call reported an application-level failure. The error originates in host Service code or in the capability routing layer, not in mruby script logic or the Wasm engine.

`ServiceError` is raised when a panic envelope with `origin == "service"` reaches the host — meaning the mruby script executed a Service dispatch that failed and the failure was not rescued within the script.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-11 | A bound Service method raises a Ruby exception during dispatch; the exception propagates through the dispatch response as `status=1`, error `type="runtime"`, and the mruby script does not rescue it | B-12 — Transport dispatch |
| E-12 | The dispatch `target` path (e.g., `"<Namespace>::<Member>"`) does not match any registered Member; error `type="undefined"` returned; mruby script does not rescue it | B-07, B-12 — undefined member |
| E-13 | The dispatch `target` is a Handle ID that does not exist in the current invocation (stale Handle from a prior invocation presented as target in a new invocation); error `type="undefined"` | B-18 — stale Handle cross-invocation |
| E-15 | Service method receives arguments that fail the host-side parameter binding (e.g., unknown keyword); error `type="argument"` returned; mruby guest does not rescue it. Passing keyword arguments to a method whose signature accepts no keyword arguments is treated as a parameter binding failure (`type="argument"`, E-15), not a Ruby runtime exception (E-11). | B-12 — Transport dispatch |
| E-43 | The dispatch method resolves, on the target, to Ruby's ambient reflection / eval surface — owner in a core meta module (`BasicObject` / `Kernel` / `Object` / `Module` / `Class`) or a callable gadget type (`Proc` / `Method` / `UnboundMethod` / `Binding`) outside the callable allowlist; error `type="undefined"` returned; mruby script does not rescue it | B-42 — reflection rejection |
| E-44 | A bound Service method returns a `Binding`, `Method`, or `UnboundMethod` — directly, or extracted by the guest from a returned container Handle; the host refuses to mint a Capability Handle and the dispatch reports `type="runtime"`; the mruby script does not rescue it | B-43 — reflective gadget not wire-representable |

A guest attempting to forge a Handle from a bare integer is rejected by the guest-side wire decoder before any dispatch reaches the host; that path raises `Kobako::SandboxError` (E-10), not `ServiceError` (B-20).

When the guest wraps a Service call in `begin/rescue`, the dispatch failure is handled within the guest; no `ServiceError` reaches the host and the invocation returns normally. `Kobako::ServiceError` is raised to the Host App only when a Service failure is unrescued at the top level of the guest execution context.

E-14 is a retired anchor — permanently reserved and never reassigned (N-8).

---

### `Kobako::SetupError`

Raised by `Kobako::Sandbox.new` when the wasm runtime cannot be constructed from the configured `wasm_path` (B-01) or the Guest Binary fails the ABI version check (B-40) — before any invocation entry point runs. Construction is a setup verb, not an invocation: `SetupError` is therefore not one of the four invocation outcomes and does not pass through the two-step attribution decision, mirroring the E-16..E-18 setup-time treatment. Because no Sandbox instance is produced, the `TrapError` "discard and recreate" recovery contract does not apply — a `SetupError` reflects a deterministic artifact or environment fault, and retrying `Sandbox.new` against the same `wasm_path` fails identically until the underlying cause is fixed.

`Kobako::ModuleNotBuiltError` is the named subclass for the common, actionable case: the Guest Binary artifact has not been built yet. A Host App that only needs "the Sandbox could not be set up" can rescue `Kobako::SetupError`; one that wants to special-case the unbuilt-artifact state can rescue `Kobako::ModuleNotBuiltError` first.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-39 | `Sandbox.new` cap argument is invalid: `timeout` is non-Numeric, non-positive, or non-finite, or `memory_limit` is not a positive Integer | host pre-flight (`SandboxOptions`, before any engine work) | `ArgumentError` |
| E-40 | The Guest Binary artifact is absent at the resolved `wasm_path` — the common state on a fresh clone before `rake compile` | construction: artifact lookup | `Kobako::ModuleNotBuiltError` |
| E-41 | The Guest Binary artifact is present but the wasm runtime cannot be constructed from it: the file cannot be read, its bytes are not a valid Wasm module, or engine / linker / instantiation setup fails | construction: read / compile / instantiate | `Kobako::SetupError` |
| E-42 | The Guest Binary does not export `__kobako_abi_version`, or the export's reported value differs from the ABI version the Host Gem implements (→ [`docs/wire-codec.md`](wire-codec.md) § ABI Version) | construction: ABI version probe (B-40) | `Kobako::SetupError` |
| E-47 | `Pool.new` argument is invalid: `slots` is not a positive Integer, or `checkout_timeout` is non-Numeric, non-positive, or non-finite (`nil` is valid and waits indefinitely) | host pre-flight (`Pool.new`, before any engine work) | `ArgumentError` |

E-42's actionable remedy is rebuilding the Guest Binary against the Host Gem's ABI version.

---

### `Kobako::PoolTimeoutError`

Raised by `Kobako::Pool#with` when the checkout wait exceeds the configured `checkout_timeout` (B-47). Checkout is a pool verb, not an invocation: `PoolTimeoutError` is not one of the four invocation outcomes and does not pass through the two-step attribution decision. No Sandbox state is touched — every pooled Sandbox is exactly as the other holders left it, and retrying `#with` succeeds as soon as a holder returns its Sandbox.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-46 | `Pool#with` waited `checkout_timeout` seconds while all `slots` Sandboxes were held by other callers (B-47) | pool checkout, before any Sandbox is touched | `Kobako::PoolTimeoutError` |

---

### Registration errors (`define` / `bind`)

These error scenarios cover Namespace declaration and Member binding (B-07..B-11) and the sealing rule (B-33). All are Host App programming errors detected at setup time, before or between guest executions; they raise `ArgumentError` synchronously and do not engage the attribution pipeline.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-16 | `sandbox.define(name)` with `name` not matching the `/\A[A-Z]\w*\z/` constant pattern (B-07) | host pre-flight | `ArgumentError` |
| E-17 | `namespace.bind(name, obj)` with `name` not matching the `/\A[A-Z]\w*\z/` constant pattern (B-08) | host pre-flight | `ArgumentError` |
| E-18 | `sandbox.define` after the first invocation (`#eval` or `#run`) has sealed Service registration (B-07, B-33) | host pre-flight | `ArgumentError` |
| E-45 | `namespace.bind` after the first invocation has sealed Service registration (B-08, B-33); the existing bindings and the Frame 1 preamble of subsequent invocations are unchanged | host pre-flight | `ArgumentError` |

---

### Entrypoint dispatch errors (`#run`)

These error scenarios are specific to the `#run(target, *args, **kwargs)` entrypoint dispatch path (B-31). Host pre-flight cases raise `TypeError` or `ArgumentError` synchronously without engaging the attribution pipeline; guest-detected cases follow the standard Step 2 path and surface as `Kobako::SandboxError`.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-24 | `#run` `target` is neither Symbol nor String | host pre-flight | `TypeError` |
| E-25 | `#run` `target` (after `.to_s`) does not match `/\A[A-Z]\w*\z/` — including any `::`-segmented name | host pre-flight | `ArgumentError` |
| E-26 | The invocation envelope written to the command buffer fails to decode as msgpack or fails shape validation on guest entry | guest entry | `Kobako::SandboxError` |
| E-27 | `#run` target Symbol does not resolve to a defined constant on top-level `Object`; the guest's Panic envelope `details:` field carries the available top-level constants contributed by preloaded snippets | guest: target Symbol does not name a defined top-level constant | `Kobako::SandboxError` |
| E-28 | `#run` entrypoint constant is defined but does not respond to `#call` | guest: entrypoint constant does not respond to `#call` | `Kobako::SandboxError` |
| E-29 | `#run` `args` or `kwargs` contains a `Kobako::Handle` instance. The Handle constructor is internal to the Host Gem; legitimate Handle production paths (B-14 service return, B-34 host-side auto-wrap) live inside the wire layer and never expose a Handle object to the Host App's call site. Any Handle reaching this position is therefore forged through a non-public path and is rejected | host pre-flight | `ArgumentError` |
| E-30 | `#run` `kwargs` contains a key that is not a Symbol | host pre-flight | `ArgumentError` |
| E-31 | Host's `__kobako_alloc` returns 0 when reserving guest memory for the invocation envelope | host pre-call | `Kobako::SandboxError` |

`#run` entrypoint runtime exceptions reuse E-04 (the entrypoint's `#call` raises an unrescued Ruby exception); unrepresentable return values reuse E-06 (the entrypoint returns an object with no wire representation); `Catalog::Handles` cap exhaustion during host-side auto-wrap reuses E-07 (B-34); timeout / memory caps reuse E-19 / E-20; unrescued Service-call faults inside the entrypoint reuse E-11, E-12, E-13, E-15.

---

### Preload errors (`#preload`)

These error scenarios are specific to the `#preload` setup verb (B-32) — covering both the `code:` source form and the `binary:` bytecode form — and the sealing rule (B-33). Host pre-flight API-misuse cases raise `ArgumentError` synchronously. Content failures originating in user-supplied snippets surface as `Kobako::SandboxError`, with the `Kobako::BytecodeError` subclass reserved for `binary:` form structural failures; backtrace attribution uses the snippet's filename when one is available (always for `code:`; for `binary:` only when the bytecode carries `debug_info`).

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-32 | `#preload(code:)` source fails mruby compilation during the trial load at preload time | guest trial load | `Kobako::SandboxError` (backtrace attributed to `(snippet:Name)`) |
| E-33 | `#preload(code:)` `name:` matches the name of a `code:` form snippet already registered on the Sandbox | host pre-flight | `ArgumentError` |
| E-34 | `#preload(code:)` `name:` does not match `/\A[A-Z]\w*\z/` | host pre-flight | `ArgumentError` |
| E-35 | `#preload` is called after the first invocation (`#eval` or `#run`) — the snippet table is sealed per B-33 | host pre-flight | `ArgumentError` |
| E-36 | A preloaded snippet's top-level expression raises during replay inside a subsequent invocation. Covers both `#preload(code:)` and `#preload(binary:)` forms — `binary:` form structural failures (E-37 / E-38) are separate. | guest static load | `Kobako::SandboxError` (backtrace attributed to `(snippet:Name)` when the snippet carries a filename) |
| E-37 | `#preload(binary:)` bytecode's RITE version does not match the version the guest mruby was built against | guest replay (first invocation) | `Kobako::BytecodeError` |
| E-38 | `#preload(binary:)` bytecode body is corrupt or malformed and fails to load during snippet replay | guest replay (first invocation) | `Kobako::BytecodeError` (backtrace attributed to the bytecode's `debug_info` filename when the bytecode carries one) |

E-33 is scoped to `code:` form snippets: duplicate `code:` form names would produce ambiguous `(snippet:Name):line` attribution in backtraces, so two `code:` snippets with the same `name:` are never permitted on a single Sandbox. The host does not extract names from `binary:` form bytecode, so cross-form name collisions are not detected at preload — users who need class reopening across multiple bodies must concatenate the sources under one snippet or use distinct names per layer.

The backtrace filename `(snippet:Name)` is the locator that ties a replay failure back to the specific `#preload` call; stripped `binary:` payloads omit the frame per B-32.

Subsequent invocations on the same Sandbox replay the same bytecode into the canonical boot state (B-49) and raise the same `Kobako::BytecodeError` deterministically (B-33 seals the table). Bytecode that loads structurally but lacks `debug_info` is not a structural failure — see B-32 for its observable effect on backtrace attribution.
