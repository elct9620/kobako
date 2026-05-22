# Behavior

The behaviors below specify observable outcomes for the Sandbox object and its execution contract. Each behavior uses the form **Initial State → Operation → Result / Final State**. Error attribution (TrapError, SandboxError, ServiceError) is covered in the Error Scenarios subsection; where an error branch is noted below, refer to that subsection for full semantics.

The Sandbox exposes two synchronous invocation verbs — `#eval` (one-shot mruby source execution, B-02 / B-03 / B-06) and `#run` (entrypoint dispatch into a preloaded constant, B-31) — plus the setup verb `#preload` (snippet registration, B-32 / B-33). The four-outcome guarantee, per-invocation isolation, and two-step attribution decision apply uniformly to both invocation verbs.

The governing summary of this document — including the four-outcome guarantee for every Sandbox invocation and the two-step attribution decision — lives in `SPEC.md` § Behavior; this document is the per-anchor reference.

---

## B-01 — Construct a new Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | No `Kobako::Sandbox` instance exists. No Guest Binary is running. |
| **Operation** | `Kobako::Sandbox.new` — optionally with the following keyword arguments: `timeout:` (Numeric seconds, default `60.0`), `memory_limit:` (Integer bytes, default `1 << 20` = 1 MiB), `stdout_limit:` (Integer bytes, default `1 << 20` = 1 MiB), `stderr_limit:` (Integer bytes, default `1 << 20` = 1 MiB). Each of the four caps accepts `nil` to disable that bound. |
| **Result / Final State** | A Sandbox instance is returned. No Guest Binary is started. The stdout and stderr buffers are empty. The snippet table (B-32) is empty. The Sandbox is ready to accept setup calls (`#define`, `#preload`) and invocations (`#eval`, `#run`). |
| **Notes** | `timeout` is measured as absolute wall-clock time from the invocation entry point (`Sandbox#eval` or `Sandbox#run`); the deadline expires at `entry_time + timeout` and is checked at guest wasm safepoints. Time spent inside a Service callback executing on the host does not itself trigger a trap — no trap fires while host code runs — but the wall-clock time it consumes counts against the deadline, so a long-running callback can cause the next guest wasm safepoint to trap immediately on return. The Host App is responsible for keeping Service handler execution bounded. `memory_limit` bounds the per-invocation linear-memory delta — measured as the cumulative `memory.grow` past the linear-memory size observed when the invocation entered, so the Guest Binary's declared initial allocation and any high-water mark left by prior invocations on the same Sandbox sit outside the budget (B-02 Result, E-20). `stdout_limit` / `stderr_limit` bound per-channel output capture (B-04). Service declarations / bindings (B-07 / B-08) and snippet preloads (B-32) are permitted at any point before the first invocation; both sets are sealed simultaneously by B-33. |

---

## B-02 — Invoke `#eval(code)` from a fresh Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with zero prior invocations (no `#eval` and no `#run` call). Zero or more Members have been bound. Zero or more snippets have been preloaded (B-32). The stdout and stderr buffers are empty. |
| **Operation** | `sandbox.eval(code)` where `code` is a String of mruby source. |
| **Result / Final State** | The HandleTable counter is reset and no Handles from any prior invocation are reachable. Service bindings registered on this Sandbox remain active. Preloaded snippets (B-32) replay in insertion order before `code` executes; each snippet contributes its top-level side effects to the fresh `mrb_state`. `code` then loads with backtrace filename `(eval)`. `#eval` blocks until execution completes, up to the configured `timeout`. On success, `#eval` returns a single deserialized Ruby value — the last mruby expression of `code`. The stdout and stderr buffers contain any output written during execution, bounded by `stdout_limit` / `stderr_limit` (B-04). Per-invocation cap exhaustion surfaces as `Kobako::TimeoutError` (wall-clock `timeout` exceeded; E-19) or `Kobako::MemoryLimitError` (per-invocation `memory.grow` delta exceeds `memory_limit`; E-20), both subclasses of `Kobako::TrapError`. If `code` is `nil`, not a String, or fails compilation, `#eval` raises `Kobako::SandboxError`. |
| **Notes** | The return value semantics are detailed in B-06. Error outcomes are covered in the Error Scenarios subsection. A `code` argument that is `nil`, not a String, or fails mruby compilation results in `Kobako::SandboxError`. The first `#eval` (or first `#run`, B-31) seals the snippet table (B-33) and Service registration (B-07). |

---

## B-03 — Invoke `#eval` or `#run` on a Sandbox that has already invoked

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance that has completed one or more prior invocations (any combination of `#eval` and `#run`). Members bound before the first invocation remain registered. Snippets preloaded before the first invocation remain registered. |
| **Operation** | `sandbox.eval(code)` or `sandbox.run(target, *args, **kwargs)` — any invocation after the first. |
| **Result / Final State** | Each invocation executes in a fully isolated context, independent of all prior invocations. All capability state (Handles issued in prior invocations) is fully discarded before the new invocation begins. All Service bindings and all preloaded snippets remain active across invocations and are visible to the new invocation. `#eval` returns the last expression of its source; `#run` returns the entrypoint's `#call` return value (B-31). The stdout and stderr buffers are cleared at the start of this invocation and contain only output from this invocation; the per-channel truncation predicates (B-04) reset together with the buffers. Per-invocation cap enforcement (B-02 Result) applies identically to every invocation, regardless of verb. |
| **Notes** | A Handle issued during invocation N is not reachable during invocation N+1. This isolation guarantee is unconditional — it holds whether the previous invocation succeeded or raised an error, and applies uniformly across `#eval`/`#run` boundaries. Service bindings and preloaded snippets are never cleared between invocations; only capability state is reset. |

---

## B-04 — Read `#stdout` / `#stderr` after an invocation returns

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which `#eval` or `#run` has been called and has returned (either with a value or by raising an error). |
| **Operation** | `sandbox.stdout`, `sandbox.stderr`, `sandbox.stdout_truncated?`, or `sandbox.stderr_truncated?` — any combination, any order, any number of times. |
| **Result / Final State** | Each byte reader returns the content (as a UTF-8 String) the guest wrote to its respective output channel during the most recent invocation, up to the configured `stdout_limit` / `stderr_limit`. The buffers do not change between successive reads. The content contains no kobako protocol bytes and no truncation sentinels. When a channel's cap was reached, the host buffer ends at the cap boundary and subsequent guest writes on that channel fail or are dropped — the guest may rescue the failure or ignore it, but no further bytes reach the buffer; this does not cause the invocation to raise an error. Each truncation predicate returns `true` iff its channel hit its cap during the most recent invocation, otherwise `false`. |
| **Notes** | The buffers and the truncation predicates remain accessible after an error-raising invocation; the Host App reads them after catching the error. Per-channel byte caps are set at construction time (B-01). Truncation predicates reset together with the buffers at the start of the next invocation (B-03). |

---

## B-05 — Read `#stdout` / `#stderr` before any invocation

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which neither `#eval` nor `#run` has ever been called. |
| **Operation** | `sandbox.stdout` or `sandbox.stderr`. |
| **Result / Final State** | Each reader returns an empty String (`""`). No error is raised. |
| **Notes** | Reading either buffer before any invocation is always safe and returns an empty String. |

---

## B-06 — Return value semantics of `#eval`

This behavior refines the Result of B-02 / B-03 by specifying the exact value `#eval` produces. The return value semantics of `#run` are specified in B-31.

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance, either fresh (per B-02) or post-invocation (per B-03), with zero or more Members bound and zero or more snippets preloaded. |
| **Operation** | `sandbox.eval(code)` — same invocation as B-02 / B-03. |
| **Result / Final State** | When the guest completes without raising `Kobako::TrapError`, `#eval` returns the deserialized Ruby value of the last mruby expression of `code`. If the last expression evaluates to `nil` (including a `code` with no explicit return expression), `#eval` returns Ruby `nil`. If the last expression produces an object that cannot be returned as a Ruby value, `#eval` raises `Kobako::SandboxError`. All other error outcomes are covered in the Error Scenarios subsection. |
| **Notes** | Exactly one value is returned per `#eval` call. There is no mechanism to return multiple values or stream values. The unrepresentable-value case is attributed to the guest code (`Kobako::SandboxError`), not to the Wasm engine or a Service call. |

---

## B-07 — Declare a Namespace on a Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which no invocation (`#eval` or `#run`) has yet been called. No Namespace named `Name` exists on this Sandbox. |
| **Operation** | `sandbox.define(:Name)` where `:Name` is a Symbol matching `/\A[A-Z]\w*\z/` (Ruby constant-name form). |
| **Result / Final State** | A `Kobako::RPC::Namespace` instance is created and associated with this Sandbox under the name `Name`. The namespace has no members yet. The method returns the new `Kobako::RPC::Namespace` instance. The Sandbox's Server now tracks one additional namespace entry. |
| **Notes** | `Name` must conform to Ruby constant naming (`/\A[A-Z]\w*\z/`); a non-conforming name raises `ArgumentError` (error scenarios covered in the Error Scenarios subsection). Declarations are design-time operations: they must be made before the first invocation. Calling `sandbox.define` after `#eval` or `#run` has been invoked raises `ArgumentError`; the Sandbox remains usable for subsequent invocations with the bindings and preloaded snippets that existed at the time of the first invocation. A namespace may have zero members at declaration time; members are added via B-08. Snippet registration is governed by the sibling sealing rule B-33. |

---

## B-08 — Bind a Member to a declared Namespace

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::RPC::Namespace` instance (returned by `sandbox.define`) with no member bound under the name `MemberName`. |
| **Operation** | `namespace.bind(:MemberName, object)` where `:MemberName` matches `/\A[A-Z]\w*\z/` and `object` is any Ruby object (class, instance, or module) that responds to the methods guest code will invoke. |
| **Result / Final State** | `object` is registered as the Member named `MemberName` within the namespace. Guest code can now reach this object via the two-level path `Name::MemberName`. The method returns the `Kobako::RPC::Namespace` instance (`self`) to allow chaining. |
| **Notes** | `bind` accepts any Ruby object — class, instance, or module — uniformly; the Host App is responsible for ensuring `object` responds to the methods guest code will call. The bound object must remain valid for the lifetime of the Sandbox; the Host App is responsible for managing its lifecycle. A `MemberName` not matching the constant-name pattern raises `ArgumentError` (see the Error Scenarios subsection). |

---

## B-09 — Declare multiple Namespaces on the same Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with one or more Namespaces already declared. |
| **Operation** | `sandbox.define(:OtherName)` with a name distinct from all already-declared namespaces on this Sandbox. |
| **Result / Final State** | A new, independent `Kobako::RPC::Namespace` is created alongside the existing namespaces. Each namespace's members are accessible to guest code only via that namespace's own path (e.g., `NamespaceA::Member` and `NamespaceB::Member` are distinct paths with no cross-namespace visibility). Namespaces on different Sandbox instances are fully isolated from each other. |
| **Notes** | There is no declared upper limit on the number of namespaces per Sandbox. Each namespace name within a Sandbox must be unique (duplicate-declare behavior is specified in B-10). |

---

## B-10 — Re-declare a Namespace that already exists (idempotent define)

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with a Namespace already declared under the name `Name`. |
| **Operation** | `sandbox.define(:Name)` — same name as an existing namespace. |
| **Result / Final State** | No new namespace is created. `sandbox.define(:Name)` returns the identical `Kobako::RPC::Namespace` object previously created — the same object identity (Ruby `equal?`), not a new instance wrapping the same Server entry. All previously bound members remain in place. The Sandbox's Server is not modified. |
| **Notes** | Idempotent re-declaration allows Host Apps to retrieve an existing namespace handle without tracking it externally (e.g., in configuration code spread across multiple files). |

---

## B-11 — Attempt to bind a Member name that is already bound in the same Namespace

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::RPC::Namespace` instance with a member already bound under the name `MemberName`. |
| **Operation** | `namespace.bind(:MemberName, new_object)` — same member name as an already-bound member. |
| **Result / Final State** | `ArgumentError` is raised. The existing binding is not overwritten. The namespace's member registry is unchanged. |
| **Notes** | Duplicate binding raises `ArgumentError`; the existing binding is preserved. Error scenarios are covered in full in the Error Scenarios subsection. |

---

## B-12 — Guest-initiated RPC call dispatched to a bound Ruby object

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. A Member is bound at `Name::MemberName`. The guest holds a reference to the constant `Name::MemberName` and calls a method on it. |
| **Operation** | Guest code executes `Name::MemberName.method_name(arg1, arg2, key: value)` — a synchronous method call from within the mruby script. |
| **Result / Final State** | The Host Gem resolves the target to the Ruby object bound at `Name::MemberName` and invokes `object.public_send(:method_name, arg1, arg2, key: value)`. The Ruby return value is serialized and returned to the guest as the synchronous result of the call — from the guest's perspective, the call completes as an ordinary synchronous Ruby method invocation. |
| **Notes** | Each RPC call invokes the bound object's method exactly once. Keyword argument names travel on the wire as Symbols (→ [`docs/wire-codec.md`](wire-codec.md) § Type Mapping); the host passes them to `public_send` without further conversion. If the target path is not found in the Server, a `ServiceError` is returned to the guest (covered in the Error Scenarios subsection). |

---

## B-13 — Service method returns a primitive value

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated RPC call (B-12) has been dispatched. The bound Ruby object's method returns a value that is **wire-representable**: `nil`, Boolean, Integer, Float, String, binary String, Symbol, Array, or Hash. |
| **Operation** | The Host Gem's wire codec serializes the return value and delivers it to the guest as the RPC response. |
| **Result / Final State** | The guest receives the return value as the synchronous result of the method call, deserialized to the corresponding mruby type. The value is indistinguishable from a locally computed mruby value. No entry is added to the HandleTable. |
| **Notes** | A value is **wire-representable** if its type is one of `nil`, Boolean, Integer, Float, String, binary String, Symbol, Array of wire-representable values, or Hash with wire-representable keys and values; or another `Kobako::Handle`. The wire codec is the same codec used for `#run` return values (B-06). Values that are not wire-representable cause a `Kobako::SandboxError` (see the Error Scenarios subsection). Collections (Array, Hash) whose elements are all wire-representable are transmitted in full by value. |

---

## B-14 — Service method returns a stateful object (Host-side Handle allocation)

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated RPC call (B-12) has been dispatched. The bound Ruby object's method returns a Ruby object that is not wire-representable — for example, a session object, a connection, or any stateful host resource. |
| **Operation** | A return value is routed through the Handle allocation path if and only if its type is not wire-representable per the definition in B-13. The wire layer then automatically registers the object in the Sandbox's HandleTable. |
| **Result / Final State** | The host-side object is stored in the HandleTable under a new opaque u32 Handle ID. The guest receives a Capability Handle (an opaque integer token) as the RPC response value, not the object itself. The guest can pass this Handle as the `target` in subsequent RPC calls to invoke methods on the same host-side object. The Host App has no API to create or inspect Handles directly — Handle allocation is an internal wire-layer operation. |
| **Notes** | Handle lifecycle (per-invocation scope, ABA protection, ID limits) is specified in the Handle lifecycle behaviors (B-15–B-21). The guest cannot dereference a Handle to a Ruby value; it can only use it as a target in further RPC calls. The host→guest symmetric direction — `#run` arguments containing non-wire-representable objects — is governed by B-34, which routes through the same HandleTable allocator and lifecycle rules. |

---

## B-15 — Handle ID is allocated with a monotonically increasing counter scoped to a single invocation

| Field | Value |
|-------|-------|
| **Initial State** | An invocation (`#eval` or `#run`) has just begun. The HandleTable counter is reset to 1. No entries exist in the table. |
| **Operation** | The Host Gem's wire layer allocates a new Handle, either for a stateful return value from a Service method (B-14) or for a non-wire-representable argument supplied to `#run` (B-34). |
| **Result / Final State** | The first Handle issued in this invocation receives ID 1, the second ID 2, and so on. Each ID is unique within the invocation. The counter never wraps or reuses an ID during a single invocation. IDs are assigned in allocation order. The counter never wraps or reuses an ID; when the cap is reached, allocation fails (see B-21). ID 0 is reserved as the invalid sentinel; allocation never returns 0. |
| **Notes** | Counter and IDs are reset at the start of every invocation — IDs from invocation N are not carried into invocation N+1 (see B-18). |

---

## B-16 — Guest passes a previously-received Handle as an argument to a Service RPC call

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress. The guest holds a `Kobako::Handle` (mruby object) obtained from a prior RPC response (B-14) or from a `#run` argument auto-wrapped by the host (B-34) in the same invocation. The Handle's internal ID resolves to a live entry in the HandleTable. |
| **Operation** | Guest code invokes a method on a Member and passes the `Kobako::Handle` as one of the arguments (e.g., `Store.put(handle, value)`). |
| **Result / Final State** | The Host Gem deserializes the Handle from the wire representation, looks up its ID in the HandleTable, and passes the resolved Ruby object as the corresponding argument to the host Service method. The Service method receives the actual Ruby object, not an ID or token. The method executes and its return value follows the normal primitive (B-13) or stateful (B-14) path. |
| **Notes** | The guest never sees or manipulates the raw integer ID; it holds a `Kobako::Handle` mruby proxy object and calls methods on it or passes it as an argument. If the ID is not found or is marked disconnected, the error path is covered in the Error Scenarios subsection. |

---

## B-17 — Chained composition: Handle returned by Service A used as target in a subsequent call to Service B

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress. Service A has been called via RPC and returned a stateful object; the guest holds `handle_a` (a `Kobako::Handle` proxy). |
| **Operation** | Guest code calls a method directly on `handle_a` (e.g., `handle_a.find(id)`), using the Handle as the RPC target. The wire layer encodes `handle_a` as the `target` field of the Request. |
| **Result / Final State** | The Host Gem resolves `handle_a`'s ID against the HandleTable and invokes `public_send(:find, id)` on the host-side Ruby object that `handle_a` represents. If that call returns another stateful object, a new Handle `handle_b` is allocated and returned to the guest. Each step in the chain is an independent, synchronous RPC; there is no implicit multi-hop traversal within a single wire call. |
| **Notes** | Chain depth is unbounded within a single invocation as long as each step produces a Handle that survives to the next call. Each intermediate Handle is a first-class entry in the HandleTable and follows the same lifecycle rules as any other Handle. Every host object reachable by the guest must have been explicitly returned by a Service method; there is no implicit intermediate binding path. |

---

## B-18 — Handle issued in invocation N is presented as a target in invocation N+1

| Field | Value |
|-------|-------|
| **Initial State** | Invocation N has completed. The guest code attempts to retain a Handle ID from invocation N and presents it as the `target` in a new invocation (N+1, of either verb). At the start of invocation N+1 the HandleTable has been fully reset: all entries from invocation N (both Service-returned via B-14 and host-injected via B-34) are cleared and the counter restarted. |
| **Operation** | Guest code in invocation N+1 calls a method using the stale Handle ID as the RPC target. |
| **Result / Final State** | The HandleTable lookup for that ID returns `:undefined` — the ID does not exist in the fresh table. The stale Handle is invalid; the Host Gem treats this as an unrecognized target. The error path (the Error Scenarios subsection) is triggered. Invocation N+1 is not interrupted for other RPC calls that do not reference stale IDs. |
| **Notes** | This outcome is unconditional: no Handle survives the invocation boundary regardless of how it was allocated (B-14 service return or B-34 host-injected argument), even when invocation N and N+1 execute the same source (or dispatch the same entrypoint) with the same Service bindings. The HandleTable is reset before the Guest Binary is instantiated for invocation N+1. |

---

## B-19 — Sandbox is discarded: all Handles for that Sandbox become invalid

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Sandbox` instance exists with zero or more completed invocations. The HandleTable is owned by this Sandbox instance. |
| **Operation** | The Sandbox instance is garbage-collected or goes out of scope; Ruby reclaims it. |
| **Result / Final State** | The HandleTable and all its entries are destroyed as part of Sandbox teardown. Every Handle that was issued during any invocation on this Sandbox is permanently invalid. No Handle entry is shared with, transferred to, or reachable from any other Sandbox instance. |
| **Notes** | Handles are not reference-counted and there is no explicit `release` API for individual entries. Validity is scoped to the owning Sandbox and the specific invocation in which the Handle was issued (B-18). A Handle that was valid in a prior invocation on this Sandbox is already invalid by the time the Sandbox is collected (per B-18); Sandbox teardown simply removes the ownership root. This applies uniformly to all HandleTable entries regardless of allocation source — Service-returned (B-14) or host-injected via `#run` (B-34). |

---

## B-20 — Neither guest nor Host App can construct or dereference a Handle from a raw integer

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress (or about to begin via `#run`). Either the guest mruby has access to an arbitrary integer value (e.g., `42` or any computed integer), or the Host App holds an arbitrary integer it intends to present as a Handle. |
| **Operation** | Code on either side of the boundary attempts to use a raw integer as a Handle — for example, by constructing a `Kobako::Handle`-like object from an integer literal, or by any means other than receiving a Handle from a Service RPC response (B-14) or from a `#run` host-side auto-wrap (B-34). |
| **Result / Final State** | No valid `Kobako::Handle` object is produced from a bare integer on either side. Neither the guest mruby API nor the Host App API exposes a public constructor that converts an integer to a Handle. A raw integer presented as an RPC target does not carry the Handle wire encoding (`ext 0x01`); the host-side wire decoder rejects the malformed encoding before the value reaches the HandleTable. A `Kobako::Handle` instance fabricated on the host side through any non-public path and passed to `#run` raises `ArgumentError` at host pre-flight (E-29). |
| **Notes** | The `Kobako::Handle` class holds the u32 ID internally but does not expose it as a readable integer attribute. Handle allocation is exclusively internal to the Host Gem: the only legitimate paths to obtain a Handle instance are receiving one through a Service return value (B-14) or through `#run` host-side auto-wrap (B-34). This rule applies symmetrically — guest code that received no Handle from a Service call has no legitimate path to construct one, and Host App code outside the Host Gem has no legitimate path either. |

---

## B-21 — HandleTable exhaustion: allocation attempt beyond the ID cap

| Field | Value |
|-------|-------|
| **Initial State** | An invocation is in progress. The HandleTable counter has reached `0x7fff_ffff` (2³¹ − 1), the maximum valid Handle ID. |
| **Operation** | The Host Gem's wire layer attempts to allocate one additional Handle for a new stateful return value. |
| **Result / Final State** | The allocation fails immediately with a `Kobako::SandboxError`. The counter is not incremented, no new entry is written to the HandleTable, and no ID is silently truncated or wrapped. The error is raised to the Host App; the current invocation terminates. |
| **Notes** | The fail-fast guard makes the violation visible rather than allowing silent semantic corruption. The error path is covered in detail in the Error Scenarios subsection. |

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
| **Initial State** | A Sandbox is executing a mruby script. A Member is bound at `Name::MemberName`. |
| **Operation** | Guest code executes `Name::MemberName.method_name(arg1, ...) { |x| ... }` — a method call accompanied by a block. |
| **Result / Final State** | The Host Gem dispatches the call as in B-12, but additionally passes a yield proxy (a Ruby Proc) into the resolved Service method as its block argument. The Service method's `block_given?` returns `true`, `yield` invokes the proxy, and the proxy is also accessible as `&block` if the method declares one. The yield proxy is valid for the duration of this dispatch only. |
| **Notes** | The block itself is not transmitted as a wire value; only a single bit (`has_block`) on the Request tells the host that a block exists. The block body remains inside the guest and is invoked through B-24's yield round-trip. The yield proxy has loose Proc-style arity (extras dropped, missing args filled with `nil`); strict-arity behavior must come from a guest-side lambda, which mruby enforces during B-24. |

---

## B-24 — Service method yields to the guest-supplied block

| Field | Value |
|-------|-------|
| **Initial State** | A Service method, invoked from a guest call that supplied a block (B-23), executes on the host. `block_given?` is `true`. |
| **Operation** | The Service method invokes the yield proxy via `yield val` or `block.call(val)` — once or many times. |
| **Result / Final State** | Each invocation is a synchronous round-trip into the guest: the guest executes the block body with the supplied arguments, and the block's last expression value is returned to the Service method as the value of the `yield` expression. The Service method continues executing after each yield until it returns, raises, or is terminated by a `break` from the block (B-25). |
| **Notes** | The round-trip uses the same wasmtime synchronous re-entry model as B-12 dispatches in the other direction. The wall-clock `timeout` and `memory_limit` (B-01) apply to the combined host + guest execution; time spent inside the block counts against the deadline. An exception raised inside the block body that the Service method does not rescue propagates back to the dispatch boundary and reaches the guest as a Service-layer fault (E-11). |

---

## B-25 — Guest block uses `break val` to terminate the yielding Service method

| Field | Value |
|-------|-------|
| **Initial State** | A Service method is mid-execution after `yield val` (B-24). |
| **Operation** | The guest block executes `break val` (where the block is a non-lambda, non-orphan block — the standard form). |
| **Result / Final State** | The Service method's invocation terminates immediately as if it had `return`ed `val`. No code in the Service method body after the `yield` statement runs. The Member call in the guest code (`Name::MemberName.method_name(...) { ... }`) returns `val`. Subsequent guest code runs normally; `break` does not terminate the enclosing guest method or invocation. |
| **Notes** | This matches standard Ruby `break` semantics — `break` unwinds the most recent yielder. `break` from a deeply-nested block (multiple `Service.outer { Service.inner { break } }`) still terminates only the innermost Service method (B-28). The Service method has no opportunity to observe the break — it is unwound transparently. |

---

## B-26 — Guest block falls through or uses `next val`

| Field | Value |
|-------|-------|
| **Initial State** | A Service method is mid-execution after `yield val` (B-24). |
| **Operation** | The guest block reaches its final expression OR executes `next val` explicitly. |
| **Result / Final State** | `yield` in the Service method returns the block's value (`val` for `next val`, the last expression's value for fallthrough). The Service method continues executing the statement after `yield`. |
| **Notes** | `next` is the explicit form of "produce this iteration's value and resume the yielder"; falling off the end of a block has the same effect with the last expression's value. Both are indistinguishable from the Service method's perspective and indistinguishable from a normal C-level yield return. |

---

## B-27 — Guest block is a lambda using `break`

| Field | Value |
|-------|-------|
| **Initial State** | A Service method is mid-execution after `yield val` (B-24). The block supplied by the guest is a lambda (e.g., created via `->`, `lambda { }`, or `&:symbol`). |
| **Operation** | The lambda body executes `break val`. |
| **Result / Final State** | The lambda returns `val` to the Service method's `yield` site as the yield value. The Service method continues normally; `break` does **not** terminate the Service method when the block is a lambda. |
| **Notes** | mruby and MRI both treat lambda `break` as a silent normal return — equivalent to `next val`. This matches B-26 from the Service method's perspective. Service methods cannot distinguish "lambda block that used `break`" from "lambda block that fell through with the same final value". |

---

## B-28 — Nested dispatch frames each receive their own block

| Field | Value |
|-------|-------|
| **Initial State** | A guest block (from a Service call `Outer.run { |a| ... }`) is mid-execution, and inside its body it calls another Service with its own block (`Inner.run { |b| ... }`). |
| **Operation** | The inner Service method yields, the inner block runs, then the outer block continues. |
| **Result / Final State** | The two yield proxies are independent. The inner Service method yields to the inner block; the outer block remains untouched. A `break` from the inner block terminates only `Inner.run` (B-25); the outer block's execution resumes normally. Nesting depth is bounded only by the wasm stack budget. |
| **Notes** | Each guest dispatch frame holds at most one block reference; nested frames stack in LIFO order, matching the synchronous re-entry call structure. The Host Gem does not assign opaque identifiers to blocks — the dispatch frame itself identifies which block any given `yield` targets. |

---

## B-29 — Service method yields multiple times before returning

| Field | Value |
|-------|-------|
| **Initial State** | A Service method has been invoked with a block (B-23). |
| **Operation** | The Service method body executes `yield` multiple times (e.g., looping over a host-side collection: `items.each { |x| yield x }`). |
| **Result / Final State** | Each `yield` is an independent synchronous round-trip into the same guest block. The block body is executed once per yield with the supplied arguments. A `break` (B-25) at any iteration terminates the Service method immediately; otherwise the Service method continues to subsequent iterations. The Service method's return value (when not broken out of) is its own last expression, not the block's final value. |
| **Notes** | The block is reusable within the dispatch — there is no per-yield setup or teardown beyond the round-trip itself. Service methods that wrap host-side iteration patterns (`each`, `map`, `inject`) translate naturally: the host writes `items.each { |x| yield x }` and the guest writes `Service.run([...]) { |x| ... }`. |

---

## B-30 — Service method receives a block but never yields

| Field | Value |
|-------|-------|
| **Initial State** | A Service method has been invoked with a block (B-23). |
| **Operation** | The Service method body completes without ever invoking `yield` or `block.call`. |
| **Result / Final State** | The block is silently discarded. The Service method's return value flows back to the guest as a normal Response (B-13 or B-14). No yield round-trip occurs; the guest block body is never executed. |
| **Notes** | This matches standard Ruby semantics: passing a block to a method that ignores it has no observable effect beyond the block being constructed. Host App developers may use `block_given?` to gate behavior on whether the guest supplied a block. |

---

## B-31 — Invoke `#run(target, *args, **kwargs)` for entrypoint dispatch

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance, either fresh (per B-02) or post-invocation (per B-03). At least one snippet preloaded via B-32 defines a top-level constant named `target` that responds to `#call`. |
| **Operation** | `sandbox.run(target, *args, **kwargs)` where `target` is a Symbol or String matching `/\A[A-Z]\w*\z/`. `args` is zero or more positional arguments; `kwargs` is zero or more Symbol-keyed keyword arguments. Argument elements (positional and keyword values) may be any Ruby value — wire-representable values cross the boundary by value, and non-wire-representable values are auto-wrapped into Capability Handles per B-34. |
| **Result / Final State** | The host normalizes `target` to Symbol via `.to_sym` and applies host pre-flight checks (target type, target pattern, args / kwargs shape, Handle-forgery rejection). Non-wire-representable argument elements are routed through host-side auto-wrap (B-34) and arrive in the guest as `Kobako::Handle` proxies. Preloaded snippets (B-32) replay in insertion order. The guest then resolves the Symbol as a top-level constant on `Object` (no `::` nesting is parsed; the Symbol names a single constant on the top-level scope), confirms it responds to `#call`, and invokes `target.call(*args, opts)` where `opts` is the kwargs Hash, omitted from the argv when empty. `#run` returns the deserialized Ruby value produced by that call. Per-invocation cap enforcement, capability state reset, and stdout / stderr buffer behavior follow B-02 / B-03 identically. |
| **Notes** | The constant lookup is restricted to top-level Object — `:"Outer::Inner"` and any other multi-segment Symbol fail the pattern check (E-25) and never reach the guest. The entrypoint convention is duck-typed on `#call`: any constant whose value is a `Proc`, `Class`, `Module`, or instance responding to `#call` is acceptable. Entrypoints accept the kwargs Hash as a trailing positional parameter (`def call(req, opts = {})`, `->(req, opts) { ... }`). An empty kwargs Hash is omitted from the argv; positional-only signatures (`def call(req)`, `->(req) { ... }`) are valid for kwargs-free invocations. The first `#run` (or first `#eval`, B-02) seals the snippet table (B-33) and Service registration (B-07). Error scenarios: `target` not Symbol or String (E-24); `target` fails the constant pattern, including any `::` (E-25); envelope decode fault (E-26); entrypoint constant undefined (E-27 — the guest's panic envelope `details:` carries the available top-level constant list, scoped to constants the preloaded snippets contributed); entrypoint does not respond to `#call` (E-28); `args` or `kwargs` contains a forged `Kobako::Handle` instance obtained outside the Host Gem's allocation paths (E-29); `kwargs` key is not a Symbol (E-30); host allocation for the invocation envelope fails (E-31). Entrypoint runtime exceptions reuse E-04; unrepresentable return values reuse E-06; HandleTable cap exhaustion during host-side auto-wrap reuses E-07; unrescued Service-call faults inside the entrypoint reuse E-11..E-15; timeout / memory caps reuse E-19 / E-20. The `#run` backtrace contains no `(eval)` frame because no user source was loaded for the invocation; the trailing frame is `(snippet:Name)` (B-32). |

---

## B-32 — Preload a snippet onto a Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which no invocation (`#eval` or `#run`) has yet been called. No snippet is currently registered under the canonical name resolved by this call. |
| **Operation** | One of two forms: (a) `sandbox.preload(code: source, name: Name)` where `source` is a String of mruby source and `Name` is a Symbol or String matching `/\A[A-Z]\w*\z/`; or (b) `sandbox.preload(binary: bytecode)` where `bytecode` is a String of RITE bytecode bytes. The `binary:` form does not accept a `name:` keyword — the snippet name, when present, comes from the bytecode's embedded `debug_info` filename. |
| **Result / Final State** | The snippet is appended to the Sandbox's insertion-ordered snippet table. The `code:` form validates `Name` against the constant pattern and the source against a fresh `mrb_state` at preload time: trial-load compile errors raise `Kobako::SandboxError` (E-32) and the snippet table is not modified. The `binary:` form records the bytecode bytes verbatim into the snippet table; RITE version mismatch (E-37) and corrupt body (E-38) are detected by the guest during the first invocation's snippet replay and raise `Kobako::BytecodeError`. Bytecode compiled without a `debug_info` section is a legal `binary:` payload — the guest loads it normally and the snippet contributes its top-level effects to the fresh `mrb_state` without a backtrace filename. On successful preload, the method returns the Sandbox instance (`self`) to allow chaining. From this point on, every subsequent invocation (`#eval` or `#run`) replays the snippet — in insertion order, before any per-invocation source or entrypoint resolution — against its fresh `mrb_state`. |
| **Notes** | The canonical name is the snippet's diagnostic identity: when present, it is the filename in the loaded IREP's `debug_info` and appears in every backtrace frame from the snippet as `(snippet:Name):line`. The `code:` form sets the ccontext filename to `Name` before compile so the name is always present. The `binary:` form receives whatever filename was baked at compile time by the producing tool, including no filename at all when the bytecode was emitted without `debug_info` (e.g., `mrbc` without `-g`): frames originating in such a snippet are silently omitted from `Exception#backtrace` per upstream mruby semantics, while exception class, message, and `origin` attribution remain intact and the snippet's top-level effects on the fresh `mrb_state` are unchanged. Duplicate `code:` form names are rejected at preload time (E-33); the host does not extract or compare names from `binary:` form bytecode, so cross-form name collisions between a `code:` snippet's `Name` and a `binary:` snippet's `debug_info` filename are not detected — mruby's top-level constant reopening semantics apply during replay and users are responsible for keeping names distinct across forms. A `code:` form `Name` not matching `/\A[A-Z]\w*\z/` raises `ArgumentError` (E-34); the `binary:` form does not accept `name:` and so does not surface E-34. Calling `#preload` after the first invocation raises `ArgumentError` (E-35, governed by B-33). A snippet's top-level expressions re-execute on every invocation as a consequence of per-invocation `mrb_state` lifecycle (B-02 / B-03); when replay raises, the failure is attributed via the snippet filename when one is available (E-36 covers replay raises in either form; E-37 / E-38 cover `binary:` form structural failures). Inter-snippet dependencies (e.g., snippet B referencing a constant defined by snippet A) require A to be preloaded before B; insertion order is the contract. |

---

## B-33 — Snippet table sealing on first invocation

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with zero or more snippets preloaded via B-32 and zero or more Namespaces / Members registered via B-07 / B-08. No invocation has yet been called. |
| **Operation** | The first invocation — `sandbox.eval(...)` or `sandbox.run(...)` — is called. |
| **Result / Final State** | The snippet table becomes immutable. The snippet set replayed on every subsequent invocation is exactly the set registered at the moment of sealing, in insertion order. Any further call to `sandbox.preload(...)` raises `ArgumentError` (E-35); the existing snippet table is preserved unchanged. Service registration (B-07 / B-08) is sealed simultaneously by the same first invocation. |
| **Notes** | The first invocation seals both the Service registry (B-07) and the snippet table together. After the seal, `#define` raises `ArgumentError` (E-18) and `#preload` raises `ArgumentError` (E-35). The two registries are stored and validated independently; the sealing boundary is the only event they share. |

---

## B-34 — `#run` argument auto-wraps into a host-side Handle when not wire-representable

| Field | Value |
|-------|-------|
| **Initial State** | `#run(target, *args, **kwargs)` is invoked (B-31). At least one element of `args`, or at least one value in `kwargs`, is not wire-representable per the type set defined in B-13 — for example, a `StringIO`, an arbitrary Host App `Env` instance, or any other Ruby object whose class is outside the wire 12-entry mapping. |
| **Operation** | During Invocation envelope encoding the Host Gem walks the `args` Array and the `kwargs` Hash values; container types (Array, Hash) are walked one level at a time, and each leaf value that is not wire-representable is allocated into the Sandbox's HandleTable. The allocator returns a fresh u32 ID (B-15) which is written into the envelope as an `ext 0x01` Capability Handle in place of the original Ruby object. Wire-representable leaves pass through unchanged. |
| **Result / Final State** | The guest mruby code receives a `Kobako::Handle` proxy at each position where the host supplied a non-wire-representable argument. The proxy carries no observable Ruby value content; method calls on it dispatch back to the host through the same `method_missing` → RPC path the guest uses for Service-returned Handles (B-17). The host-side HandleTable entry remains live for the duration of the invocation and is cleared together with all other Handles at the invocation boundary (B-18 / B-19). HandleTable cap exhaustion during the walk raises `Kobako::SandboxError` at host pre-call via the same path as B-21 / E-07. |
| **Notes** | This behavior is symmetric with B-14 (Service-returned stateful objects): both directions of the boundary route non-wire-representable Ruby objects through the HandleTable allocator under identical lifecycle rules. The walk traverses Array and Hash containers but does not descend into instance variables or other internal structure of non-wire-representable leaves — once a leaf is identified as needing wrapping, its sub-structure is hidden behind the Handle. A `Kobako::Handle` value already produced internally by the Host Gem (e.g., an instance fabricated through any non-public path) is rejected at host pre-flight (E-29); auto-wrap never re-wraps an existing Handle. |

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
| E-20 | Cumulative guest `memory.grow` since invocation entry would push past the configured `memory_limit` (B-01) — the mruby image's initial allocation and prior invocations' watermark are folded into the per-invocation baseline rather than the budget | Wasm engine reports a memory-cap trap; Step 1 fires | `Kobako::MemoryLimitError` |

**Cross-references:** E-02 and E-03 are the wire-violation fallback paths invoked by any malformed Guest Binary output. B-21 (Handle counter exhaustion) raises `Kobako::SandboxError`, not `TrapError`. E-19 fires only at guest wasm safepoints — a Service callback running on the host cannot itself trigger E-19 — but the wall-clock time consumed by host callbacks counts against the `timeout` budget (B-01 Notes).

---

### `Kobako::SandboxError`

Raised when the guest execution environment ran to completion but the overall execution failed due to a protocol fault, a mruby runtime error, or a Host Gem–side wire decode failure. The guest Wasm instance is retired normally; the sandbox infrastructure itself is intact.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-04 | Guest mruby script raises an uncaught exception (e.g., `RuntimeError`, `NoMethodError`) that reaches the top level of `__kobako_run` | B-02, B-03 — script execution |
| E-05 | The guest fails to compile the source supplied to `#eval` before any execution begins | B-02 — fresh invocation |
| E-06 | `#run` last-expression result has no wire representation (e.g., a raw mruby `Object` with no MessagePack encoding); outcome tag `0x01` is present but the value field fails to decode | B-06 — return value semantics |
| E-07 | Handle issuance for the returned object fails because the per-invocation Handle counter has reached `0x7fff_ffff` (2³¹ − 1) | B-21 — Handle counter exhaustion |
| E-08 | Outcome tag is `0x02` (panic) and the panic envelope is malformed or missing required fields | Step 2 attribution table |
| E-09 | Outcome tag is `0x01` (result) and the result envelope is malformed or fails MessagePack parse | Step 2 attribution; B-06 fallback |
| E-10 | Guest presents an invalid wire payload as an RPC argument (e.g., a raw integer where a Capability Handle ext type `0x01` is required) | B-20 — guest cannot forge Handles |
| E-16 | Host App calls `sandbox.define(name)` with `name` not matching `/\A[A-Z]\w*\z/` constant pattern | B-07 — invalid Namespace Name |
| E-17 | Host App calls `namespace.bind(name, obj)` with `name` not matching `/\A[A-Z]\w*\z/` constant pattern | B-08 — invalid MemberName |
| E-18 | Host App calls `sandbox.define` after `#run` has already been invoked on this Sandbox | B-07 — define-after-run |
| E-21 | Guest block uses `return val` while its enclosing method is still on the guest call stack (non-lambda, non-orphan Proc); the unwind crosses the host yield boundary, which is unrepresentable on the wire | B-24 — yield round-trip |
| E-22 | Guest block returns a value that has no MessagePack wire representation per [`docs/wire-codec.md`](wire-codec.md) § Type Mapping | B-24 — yield round-trip |
| E-23 | Host Service method invokes its yield proxy after the originating dispatch frame has returned (e.g., the Service stored the block via `&block` and called it from a later dispatch or post-dispatch host code) | B-23 — yield-proxy scope |

`sandbox.define(:Name)` where `:Name` does not match `/\A[A-Z]\w*\z/` raises `ArgumentError` (B-07, E-16). `namespace.bind(:MemberName, obj)` when `MemberName` does not match the constant-name pattern raises `ArgumentError` (B-08, E-17). Calling `sandbox.define` after `#run` raises `ArgumentError` (B-07, E-18). All three are Host App programming errors detected at setup time before or between guest executions; they do not go through the attribution pipeline and are not classified as `SandboxError`.

---

### `Kobako::ServiceError`

Raised when the guest execution environment ran to completion, the mruby script itself did not crash, but a Service capability call reported an application-level failure. The error originates in host Service code or in the capability routing layer, not in mruby script logic or the Wasm engine.

`ServiceError` is raised when a panic envelope with `origin == "service"` reaches the host — meaning the mruby script executed a Service RPC that failed and the failure was not rescued within the script.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-11 | A bound Service method raises a Ruby exception during dispatch; the exception propagates through the RPC response as `status=1`, error `type="runtime"`, and the mruby script does not rescue it | B-12 — RPC dispatch |
| E-12 | The RPC `target` path (e.g., `"Name::MemberName"`) does not match any registered Member; error `type="undefined"` returned; mruby script does not rescue it | B-07, B-12 — undefined member |
| E-13 | The RPC `target` is a Handle ID that does not exist in the current invocation (stale Handle from a prior invocation presented as target in a new invocation); error `type="undefined"` | B-18 — stale Handle cross-invocation |
| E-14 | The RPC `target` Handle ID resolves to the `:disconnected` sentinel in the HandleTable; error `type="disconnected"` | B-16 — Handle referencing |
| E-15 | Service method receives arguments that fail the host-side parameter binding (e.g., unknown keyword); error `type="argument"` returned; mruby guest does not rescue it. Passing keyword arguments to a method whose signature accepts no keyword arguments is treated as a parameter binding failure (`type="argument"`, E-15), not a Ruby runtime exception (E-11). | B-12 — RPC dispatch |

A Handle ID from invocation N presented as an RPC target in invocation N+1 produces `type="undefined"` because the Handle table is fully reset at the start of each invocation; this reaches the host as `Kobako::ServiceError` if the guest does not rescue the error response (B-18). A guest attempting to forge a Handle from a bare integer is rejected by the guest-side wire decoder before any RPC reaches the host; that path raises `Kobako::SandboxError` (E-10), not `ServiceError` (B-20).

When the guest wraps a Service call in `begin/rescue`, the RPC failure is handled within the guest; no `ServiceError` reaches the host and the invocation returns normally. `Kobako::ServiceError` is raised to the Host App only when a Service failure is unrescued at the top level of the guest execution context.

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

`#run` entrypoint runtime exceptions reuse E-04 (the entrypoint's `#call` raises an unrescued Ruby exception); unrepresentable return values reuse E-06 (the entrypoint returns an object with no wire representation); timeout / memory caps reuse E-19 / E-20; unrescued Service-call faults inside the entrypoint reuse E-11..E-15.

E-24, E-25, E-29, and E-30 are Host App programming errors detected before the invocation crosses into the guest; they raise standard Ruby exceptions (`TypeError` / `ArgumentError`) and do not go through the attribution pipeline, mirroring the E-16..E-18 treatment.

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
| E-38 | `#preload(binary:)` bytecode body is corrupt or malformed and fails to load on a fresh `mrb_state` | guest replay (first invocation) | `Kobako::BytecodeError` (backtrace attributed to the bytecode's `debug_info` filename when the bytecode carries one) |

E-33 is scoped to `code:` form snippets: duplicate `code:` form names would produce ambiguous `(snippet:Name):line` attribution in backtraces, so two `code:` snippets with the same `name:` are never permitted on a single Sandbox. The host does not extract names from `binary:` form bytecode, so cross-form name collisions are not detected at preload — users who need class reopening across multiple bodies must concatenate the sources under one snippet or use distinct names per layer.

E-32 and E-36 surface as `Kobako::SandboxError` because they originate in user-supplied snippet content. The backtrace filename `(snippet:Name)` is the locator that ties the failure back to the specific `#preload` call when one is available — always for the `code:` form (the host writes the name into the compile ccontext) and for `binary:` payloads carrying `debug_info`. For stripped `binary:` payloads the snippet frame is omitted from the backtrace per B-32; the failure's class, message, and `origin` attribution remain unchanged.

E-37 and E-38 surface as `Kobako::BytecodeError` because they originate in the structural content of supplied bytecode (version mismatch or corrupt body). Both are detected during the first invocation's snippet replay against a fresh `mrb_state`. The snippet table is sealed by this first invocation per B-33; subsequent invocations on the same Sandbox replay the same bytecode against a new fresh `mrb_state` and raise the same `Kobako::BytecodeError` deterministically. Bytecode that loads structurally but lacks a `debug_info` section is not a structural failure — see B-32 for its observable effect on backtrace attribution.

E-33, E-34, and E-35 are Host App programming errors detected before the snippet is registered (E-33 / E-34) or before the invocation reaches the guest (E-35); all three raise `ArgumentError` and do not engage the attribution pipeline.
