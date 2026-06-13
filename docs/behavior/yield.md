# Yield — block re-entry

Passing a guest block to a Service method and the synchronous yield round-trip back into the guest. The governing summary lives in [`SPEC.md`](../../SPEC.md)
§ Behavior; this file is the per-anchor reference. `B-xx` anchors are global
and append-only across the corpus (N-8).

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
