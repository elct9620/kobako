# Behavior

The behaviors below specify observable outcomes for the Sandbox object and its execution contract. Each behavior uses the form **Initial State → Operation → Result / Final State**. Error attribution (TrapError, SandboxError, ServiceError) is covered in the Error Scenarios subsection; where an error branch is noted below, refer to that subsection for full semantics.

The governing summary of this document — including the four-outcome guarantee for every `Sandbox#run` and the two-step attribution decision — lives in `SPEC.md` § Behavior; this document is the per-anchor reference.

---

## B-01 — Construct a new Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | No `Kobako::Sandbox` instance exists. No Guest Binary is running. |
| **Operation** | `Kobako::Sandbox.new` — optionally with the following keyword arguments: `timeout:` (Numeric seconds, default `60.0`), `memory_limit:` (Integer bytes, default `5 << 20` = 5 MiB), `stdout_limit:` (Integer bytes, default `1 << 20` = 1 MiB), `stderr_limit:` (Integer bytes, default `1 << 20` = 1 MiB). Each of the four caps accepts `nil` to disable that bound. |
| **Result / Final State** | A Sandbox instance is returned. No Guest Binary is started. The stdout and stderr buffers are empty. The Sandbox is ready to accept `#run` calls. |
| **Notes** | `timeout` is measured as absolute wall-clock time from `Sandbox#run` invocation; the deadline expires at `entry_time + timeout` and is checked at guest wasm safepoints. Time spent inside a Service callback executing on the host does not itself trigger a trap — no trap fires while host code runs — but the wall-clock time it consumes counts against the deadline, so a long-running callback can cause the next guest wasm safepoint to trap immediately on return. The Host App is responsible for keeping Service handler execution bounded. `memory_limit` bounds guest linear memory growth (B-02 Result, E-20). `stdout_limit` / `stderr_limit` bound per-channel output capture (B-04). Service declarations and bindings are permitted at any point before the first `#run` call. |

---

## B-02 — Invoke `#run(script)` from a fresh Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with zero prior `#run` calls. Zero or more Members have been bound. The stdout and stderr buffers are empty. |
| **Operation** | `sandbox.run(script_string)` where `script_string` is a valid mruby script. |
| **Result / Final State** | Each `#run` call executes with a fresh capability state — the HandleTable counter is reset and no Handles from prior runs are reachable. Service bindings registered on this Sandbox remain active across runs. `#run` blocks until execution completes, up to the configured `timeout`. On success, `#run` returns a single deserialized Ruby value — the script's last expression. The stdout and stderr buffers contain any output the script wrote during execution, bounded by `stdout_limit` / `stderr_limit` (B-04). Per-run cap exhaustion surfaces as `Kobako::TimeoutError` (wall-clock `timeout` exceeded; E-19) or `Kobako::MemoryLimitError` (guest `memory.grow` exceeds `memory_limit`; E-20), both subclasses of `Kobako::TrapError`. If `script_string` is `nil`, not a String, or fails compilation, `#run` raises `Kobako::SandboxError`. |
| **Notes** | The return value semantics are detailed in B-06. Error outcomes are covered in the Error Scenarios subsection. A `script_string` that is `nil`, not a String, or fails mruby compilation results in `Kobako::SandboxError`. |

---

## B-03 — Invoke `#run(script)` on a Sandbox that has already run

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance that has completed one or more prior `#run` calls. Members bound before the first `#run` remain registered. |
| **Operation** | `sandbox.run(script_string)` — any invocation after the first. |
| **Result / Final State** | Each `#run` call executes in a fully isolated context, independent of all prior invocations. All capability state (Handles issued in prior runs) from previous runs is fully discarded before the new run begins. All Service bindings registered on this Sandbox at any point remain active across runs and are visible to the new run. `#run` returns the new script's last expression. The stdout and stderr buffers are cleared at the start of this run and contain only output from this invocation; the per-channel truncation predicates (B-04) reset together with the buffers. Per-run cap enforcement (B-02 Result) applies identically to every `#run` invocation. |
| **Notes** | A Handle issued during run N is not reachable during run N+1. This isolation guarantee is unconditional — it holds whether the previous run succeeded or raised an error. Service bindings are never cleared between runs; only capability state is reset. |

---

## B-04 — Read `#stdout` / `#stderr` after `#run` returns

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which `#run` has been called and has returned (either with a value or by raising an error). |
| **Operation** | `sandbox.stdout`, `sandbox.stderr`, `sandbox.stdout_truncated?`, or `sandbox.stderr_truncated?` — any combination, any order, any number of times. |
| **Result / Final State** | Each byte reader returns the content (as a UTF-8 String) the guest script wrote to its respective output channel during the most recent `#run` invocation, up to the configured `stdout_limit` / `stderr_limit`. The buffers do not change between successive reads. The content contains no kobako protocol bytes and no truncation sentinels. When a channel's cap was reached, the host buffer ends at the cap boundary and subsequent guest writes on that channel fail or are dropped — the script may rescue the failure or ignore it, but no further bytes reach the buffer; this does not cause `#run` to raise an error. Each truncation predicate returns `true` iff its channel hit its cap during the most recent `#run`, otherwise `false`. |
| **Notes** | The buffers and the truncation predicates remain accessible after an error-raising `#run`; the Host App reads them after catching the error. Per-channel byte caps are set at construction time (B-01). Truncation predicates reset together with the buffers at the start of the next `#run` (B-03). |

---

## B-05 — Read `#stdout` / `#stderr` before any `#run` call

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which `#run` has never been called. |
| **Operation** | `sandbox.stdout` or `sandbox.stderr`. |
| **Result / Final State** | Each reader returns an empty String (`""`). No error is raised. |
| **Notes** | Reading either buffer before `#run` is always safe and returns an empty String. |

---

## B-06 — Return value semantics of `#run`

This behavior refines the Result of B-02 / B-03 by specifying the exact value `#run` produces.

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance, either fresh (per B-02) or post-run (per B-03), with zero or more Members bound. |
| **Operation** | `sandbox.run(script_string)` — same invocation as B-02 / B-03. |
| **Result / Final State** | When the guest script completes without raising `Kobako::TrapError`, `#run` returns the deserialized Ruby value of the script's last mruby expression. If the last expression evaluates to `nil` (including scripts with no explicit return expression), `#run` returns Ruby `nil`. If the script's last expression produces an object that cannot be returned as a Ruby value, `#run` raises `Kobako::SandboxError`. All other error outcomes are covered in the Error Scenarios subsection. |
| **Notes** | Exactly one value is returned per `#run` call. There is no mechanism for a script to return multiple values or stream values. This error is attributed to the script (`Kobako::SandboxError`), not to the Wasm engine or a Service call. |

---

## B-07 — Declare a Namespace on a Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which `#run` has not yet been called. No Namespace named `Name` exists on this Sandbox. |
| **Operation** | `sandbox.define(:Name)` where `:Name` is a Symbol matching `/\A[A-Z]\w*\z/` (Ruby constant-name form). |
| **Result / Final State** | A `Kobako::RPC::Namespace` instance is created and associated with this Sandbox under the name `Name`. The namespace has no members yet. The method returns the new `Kobako::RPC::Namespace` instance. The Sandbox's Server now tracks one additional namespace entry. |
| **Notes** | `Name` must conform to Ruby constant naming (`/\A[A-Z]\w*\z/`); a non-conforming name raises `ArgumentError` (error scenarios covered in the Error Scenarios subsection). Declarations are design-time operations: they must be made before `#run` is first called. Calling `sandbox.define` after `#run` has been invoked raises `ArgumentError`; the Sandbox remains usable for subsequent `#run` calls with the bindings that existed at the time of the first `#run`. A namespace may have zero members at declaration time; members are added via B-08. |

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
| **Initial State** | A Sandbox executing a mruby script. A Member is bound at `Name::MemberName`. The guest script holds a reference to the constant `Name::MemberName` and calls a method on it. |
| **Operation** | Guest code executes `Name::MemberName.method_name(arg1, arg2, key: value)` — a synchronous method call from within the mruby script. |
| **Result / Final State** | The Host Gem resolves the target to the Ruby object bound at `Name::MemberName` and invokes `object.public_send(:method_name, arg1, arg2, key: value)`. The Ruby return value is serialized and returned to the guest as the synchronous result of the call — from the guest script's perspective, the call completes as an ordinary synchronous Ruby method invocation. |
| **Notes** | Each RPC call invokes the bound object's method exactly once. Keyword argument names travel on the wire as Symbols (→ [`docs/wire-codec.md`](wire-codec.md) § Type Mapping); the host passes them to `public_send` without further conversion. If the target path is not found in the Server, a `ServiceError` is returned to the guest (covered in the Error Scenarios subsection). |

---

## B-13 — Service method returns a primitive value

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated RPC call (B-12) has been dispatched. The bound Ruby object's method returns a value that is **wire-representable**: `nil`, Boolean, Integer, Float, String, binary String, Symbol, Array, or Hash. |
| **Operation** | The Host Gem's wire codec serializes the return value and delivers it to the guest as the RPC response. |
| **Result / Final State** | The guest script receives the return value as the synchronous result of the method call, deserialized to the corresponding mruby type. The value is indistinguishable from a locally computed mruby value. No entry is added to the HandleTable. |
| **Notes** | A value is **wire-representable** if its type is one of `nil`, Boolean, Integer, Float, String, binary String, Symbol, Array of wire-representable values, or Hash with wire-representable keys and values; or another `Kobako::RPC::Handle`. The wire codec is the same codec used for `#run` return values (B-06). Values that are not wire-representable cause a `Kobako::SandboxError` (see the Error Scenarios subsection). Collections (Array, Hash) whose elements are all wire-representable are transmitted in full by value. |

---

## B-14 — Service method returns a stateful object (Host-side Handle allocation)

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated RPC call (B-12) has been dispatched. The bound Ruby object's method returns a Ruby object that is not wire-representable — for example, a session object, a connection, or any stateful host resource. |
| **Operation** | A return value is routed through the Handle allocation path if and only if its type is not wire-representable per the definition in B-13. The wire layer then automatically registers the object in the Sandbox's HandleTable. |
| **Result / Final State** | The host-side object is stored in the HandleTable under a new opaque u32 Handle ID. The guest receives a Capability Handle (an opaque integer token) as the RPC response value, not the object itself. The guest can pass this Handle as the `target` in subsequent RPC calls to invoke methods on the same host-side object. The Host App has no API to create or inspect Handles directly — Handle allocation is an internal wire-layer operation. |
| **Notes** | Handle lifecycle (per-`#run` scope, ABA protection, ID limits) is specified in the Handle lifecycle behaviors (B-15–B-21). The guest cannot dereference a Handle to a Ruby value; it can only use it as a target in further RPC calls. |

---

## B-15 — Handle ID is allocated with a monotonically increasing counter scoped to a single `#run`

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation has just begun. The HandleTable counter is reset to 1. No entries exist in the table. |
| **Operation** | The Host Gem's wire layer allocates a new Handle for a stateful return value (B-14). |
| **Result / Final State** | The first Handle issued in this run receives ID 1, the second ID 2, and so on. Each ID is unique within the run. The counter never wraps or reuses an ID during a single `#run`. IDs are assigned in allocation order. The counter never wraps or reuses an ID; when the cap is reached, allocation fails (see B-21). ID 0 is reserved as the invalid sentinel; allocation never returns 0. |
| **Notes** | Counter and IDs are reset at the start of every `#run` — IDs from run N are not carried into run N+1 (see B-18). |

---

## B-16 — Guest passes a previously-received Handle as an argument to a Service RPC call

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation is in progress. The guest holds a `Kobako::RPC::Handle` (mruby object) obtained from a prior RPC response in the same run. The Handle's internal ID resolves to a live entry in the HandleTable. |
| **Operation** | Guest code invokes a method on a Member and passes the `Kobako::RPC::Handle` as one of the arguments (e.g., `Store.put(handle, value)`). |
| **Result / Final State** | The Host Gem deserializes the Handle from the wire representation, looks up its ID in the HandleTable, and passes the resolved Ruby object as the corresponding argument to the host Service method. The Service method receives the actual Ruby object, not an ID or token. The method executes and its return value follows the normal primitive (B-13) or stateful (B-14) path. |
| **Notes** | The guest never sees or manipulates the raw integer ID; it holds a `Kobako::RPC::Handle` mruby proxy object and calls methods on it or passes it as an argument. If the ID is not found or is marked disconnected, the error path is covered in the Error Scenarios subsection. |

---

## B-17 — Chained composition: Handle returned by Service A used as target in a subsequent call to Service B

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation is in progress. Service A has been called via RPC and returned a stateful object; the guest holds `handle_a` (a `Kobako::RPC::Handle` proxy). |
| **Operation** | Guest code calls a method directly on `handle_a` (e.g., `handle_a.find(id)`), using the Handle as the RPC target. The wire layer encodes `handle_a` as the `target` field of the Request. |
| **Result / Final State** | The Host Gem resolves `handle_a`'s ID against the HandleTable and invokes `public_send(:find, id)` on the host-side Ruby object that `handle_a` represents. If that call returns another stateful object, a new Handle `handle_b` is allocated and returned to the guest. Each step in the chain is an independent, synchronous RPC; there is no implicit multi-hop traversal within a single wire call. |
| **Notes** | Chain depth is unbounded within a single `#run` as long as each step produces a Handle that survives to the next call. Each intermediate Handle is a first-class entry in the HandleTable and follows the same lifecycle rules as any other Handle. Every host object reachable by the guest must have been explicitly returned by a Service method; there is no implicit intermediate binding path. |

---

## B-18 — Handle issued in run N is presented as a target in run N+1

| Field | Value |
|-------|-------|
| **Initial State** | Run N has completed. The guest (or a script) attempts to retain a Handle ID from run N and presents it as the `target` in a new `#run` invocation (run N+1). At the start of run N+1 the HandleTable has been fully reset: all entries from run N are cleared and the counter restarted. |
| **Operation** | Guest code in run N+1 calls a method using the stale Handle ID as the RPC target. |
| **Result / Final State** | The HandleTable lookup for that ID returns `:undefined` — the ID does not exist in the fresh table. The stale Handle is invalid; the Host Gem treats this as an unrecognized target. The error path (the Error Scenarios subsection) is triggered. Run N+1 is not interrupted for other RPC calls that do not reference stale IDs. |
| **Notes** | This outcome is unconditional: even if run N and run N+1 execute the same script with the same Service bindings, no Handle survives the `#run` boundary. The HandleTable is reset before the Guest Binary is instantiated for run N+1. |

---

## B-19 — Sandbox is discarded: all Handles for that Sandbox become invalid

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Sandbox` instance exists with zero or more completed `#run` invocations. The HandleTable is owned by this Sandbox instance. |
| **Operation** | The Sandbox instance is garbage-collected or goes out of scope; Ruby reclaims it. |
| **Result / Final State** | The HandleTable and all its entries are destroyed as part of Sandbox teardown. Every Handle that was issued during any `#run` on this Sandbox is permanently invalid. No Handle entry is shared with, transferred to, or reachable from any other Sandbox instance. |
| **Notes** | Handles are not reference-counted and there is no explicit `release` API for individual entries. Validity is scoped to the owning Sandbox and the specific `#run` in which the Handle was issued (B-18). A Handle that was valid in a prior `#run` on this Sandbox is already invalid by the time the Sandbox is collected (per B-18); Sandbox teardown simply removes the ownership root. |

---

## B-20 — Guest cannot construct or dereference a Handle from a raw integer

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation is in progress. The guest mruby script has access to an arbitrary integer value (e.g., `42` or any computed integer). |
| **Operation** | Guest code attempts to use a raw integer as a Handle target for an RPC call — for example, by constructing a `Kobako::RPC::Handle`-like object from an integer literal, or by any means other than receiving a Handle from a prior RPC response. |
| **Result / Final State** | No valid `Kobako::RPC::Handle` mruby object is produced from a bare integer. The guest mruby API does not expose a constructor that converts an integer to a Handle. A raw integer presented as an RPC target does not carry the Handle wire encoding (`ext 0x01`); the host-side wire decoder rejects the malformed encoding before the value reaches the HandleTable. The error path is covered in the Error Scenarios subsection. |
| **Notes** | The `Kobako::RPC::Handle` mruby class holds the u32 ID internally but does not expose it as a readable integer attribute. This prevents guest code from forging capability references. Guest code that received no Handle from a Service call has no legitimate path to construct one. |

---

## B-21 — HandleTable exhaustion: allocation attempt beyond the ID cap

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation is in progress. The HandleTable counter has reached `0x7fff_ffff` (2³¹ − 1), the maximum valid Handle ID. |
| **Operation** | The Host Gem's wire layer attempts to allocate one additional Handle for a new stateful return value. |
| **Result / Final State** | The allocation fails immediately with a `Kobako::SandboxError`. The counter is not incremented, no new entry is written to the HandleTable, and no ID is silently truncated or wrapped. The error is raised to the Host App; the current `#run` terminates. |
| **Notes** | The fail-fast guard makes the violation visible rather than allowing silent semantic corruption. The error path is covered in detail in the Error Scenarios subsection. |

---

## B-22 — Distinct Sandboxes on distinct Threads execute independently

| Field | Value |
|-------|-------|
| **Initial State** | Two or more Ruby Threads exist within the same process. The Host App has constructed one `Kobako::Sandbox` per Thread (honoring the input assumption in Scope → Interaction). |
| **Operation** | Each Thread invokes `#run` only on its own owning Sandbox; no Sandbox is shared across Threads. |
| **Result / Final State** | Each `#run` executes independently — capability state, Handle IDs, and capture buffers are scoped per Sandbox and never observed by another Thread's run. The wasmtime Engine and the compiled Module for `data/kobako.wasm` are shared at process scope: the first Sandbox in the process pays the Engine init and Module compile cost; subsequent Sandboxes in any Thread amortize against that shared state. |
| **Notes** | Aggregate throughput across Threads is bounded by Ruby's GVL — Kobako's native extension does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is serialized. Ruby-side setup (preamble pack, buffer init) can overlap across Threads, giving modest but non-linear scaling under contention. The Host App is responsible for the one-Thread-per-Sandbox invariant; Kobako provides no locking on `#run` and concurrent invocations on the same Sandbox are unsupported (Scope → Interaction). |

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
| **Result / Final State** | The Service method's invocation terminates immediately as if it had `return`ed `val`. No code in the Service method body after the `yield` statement runs. The Member call in the guest script (`Name::MemberName.method_name(...) { ... }`) returns `val`. Subsequent guest code runs normally; `break` does not terminate the enclosing guest method or script. |
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

## Error Scenarios

Every `Sandbox#run` invocation terminates in exactly one of four outcomes: a return value, `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError`. Attribution is determined by a two-step decision applied after `__kobako_run` returns:

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

Raised when the Wasm execution engine crashes, when the wire layer detects a structural violation that signals a corrupted guest execution environment, or when a configured per-run cap is exceeded. The base class `Kobako::TrapError` covers engine and wire-violation traps; the named subclasses `Kobako::TimeoutError` and `Kobako::MemoryLimitError` cover the configured-cap cases. After any TrapError (base class or subclass), the Sandbox is considered unrecoverable; Host Apps should discard and recreate it before the next execution.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-01 | Wasm engine trap: `unreachable` instruction, stack overflow, or import signature mismatch | Wasm engine reports a native trap; Step 1 fires | `Kobako::TrapError` |
| E-02 | Guest exited without writing any outcome bytes (`len == 0`) | Step 2: zero-length outcome bytes; wire violation fallback | `Kobako::TrapError` |
| E-03 | Outcome first byte is an unknown tag (not `0x01` or `0x02`) | Step 2: unrecognized tag; wire violation fallback | `Kobako::TrapError` |
| E-19 | Absolute wall-clock time since `Sandbox#run` invocation reached the configured `timeout` and a guest wasm safepoint was hit thereafter (B-01) | Wasm engine reports a wall-clock interrupt at the first guest wasm safepoint after the absolute deadline; Step 1 fires | `Kobako::TimeoutError` |
| E-20 | Guest `memory.grow` would exceed the configured `memory_limit` (B-01) | Wasm engine reports a memory-cap trap; Step 1 fires | `Kobako::MemoryLimitError` |

**Cross-references:** E-02 and E-03 are the wire-violation fallback paths invoked by any malformed Guest Binary output. B-21 (Handle counter exhaustion) raises `Kobako::SandboxError`, not `TrapError`. E-19 fires only at guest wasm safepoints — a Service callback running on the host cannot itself trigger E-19 — but the wall-clock time consumed by host callbacks counts against the `timeout` budget (B-01 Notes).

---

### `Kobako::SandboxError`

Raised when the guest execution environment ran to completion but the overall execution failed due to a protocol fault, a mruby runtime error, or a Host Gem–side wire decode failure. The guest Wasm instance is retired normally; the sandbox infrastructure itself is intact.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-04 | Guest mruby script raises an uncaught exception (e.g., `RuntimeError`, `NoMethodError`) that reaches the top level of `__kobako_run` | B-02, B-03 — script execution |
| E-05 | Guest boot script fails to load or compile the user script (`mrb_load_string` error before execution begins) | B-02 — fresh run |
| E-06 | `#run` last-expression result has no wire representation (e.g., a raw mruby `Object` with no MessagePack encoding); outcome tag `0x01` is present but the value field fails to decode | B-06 — return value semantics |
| E-07 | Handle issuance for the returned object fails because the per-run Handle counter has reached `0x7fff_ffff` (2³¹ − 1) | B-21 — Handle counter exhaustion |
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
| E-13 | The RPC `target` is a Handle ID that does not exist in the current run (stale Handle from a prior run presented as target in a new run); error `type="undefined"` | B-18 — stale Handle cross-run |
| E-14 | The RPC `target` Handle ID resolves to the `:disconnected` sentinel in the HandleTable; error `type="disconnected"` | B-16 — Handle referencing |
| E-15 | Service method receives arguments that fail the host-side parameter binding (e.g., unknown keyword); error `type="argument"` returned; mruby script does not rescue it. Passing keyword arguments to a method whose signature accepts no keyword arguments is treated as a parameter binding failure (`type="argument"`, E-15), not a Ruby runtime exception (E-11). | B-12 — RPC dispatch |

A Handle ID from run N presented as an RPC target in run N+1 produces `type="undefined"` because the Handle table is fully reset at the start of each `#run`; this reaches the host as `Kobako::ServiceError` if the script does not rescue the error response (B-18). A guest attempting to forge a Handle from a bare integer is rejected by the guest-side wire decoder before any RPC reaches the host; that path raises `Kobako::SandboxError` (E-10), not `ServiceError` (B-20).

When a guest script wraps a Service call in `begin/rescue`, the RPC failure is handled within the script; no `ServiceError` reaches the host and `#run` returns normally. `Kobako::ServiceError` is raised to the Host App only when a Service failure is unrescued at the top level of the script.
