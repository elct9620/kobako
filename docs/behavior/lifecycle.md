# Lifecycle — construction, `#eval`, output, usage

Sandbox construction, the `#eval` one-shot invocation, output capture, and per-invocation usage. The governing summary lives in [`SPEC.md`](../../SPEC.md)
§ Behavior; this file is the per-anchor reference. `B-xx` anchors are global
and append-only across the corpus (N-8).

## B-01 — Construct a new Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | No `Kobako::Sandbox` instance exists. No Guest Binary is running. |
| **Operation** | `Kobako::Sandbox.new` — optionally with the following keyword arguments: `timeout:` (Numeric seconds, default `60.0`), `memory_limit:` (Integer bytes, default `1 << 20` = 1 MiB), `stdout_limit:` (Integer bytes, default `1 << 20` = 1 MiB), `stderr_limit:` (Integer bytes, default `1 << 20` = 1 MiB), `profile:` (Symbol isolation profile the runtime builds, default `:hermetic` — B-54). Each of the four caps accepts `nil` to disable that bound; `profile` has no `nil` form (the weakest posture is requested explicitly as `:permissive`). |
| **Result / Final State** | A Sandbox instance is returned; no invocation entry point runs. The stdout and stderr buffers are empty and the snippet table (B-32) is empty. The Sandbox is ready to accept setup calls — declaring Namespaces, binding Members, and preloading snippets (B-07 / B-08 / B-32), permitted until the first invocation seals them (B-33) — and invocations (`#eval`, `#run`). Construction performs the one-time wasm runtime setup from `wasm_path`, the ABI version probe (B-40), and the isolation-profile floor check (B-54); an invalid option argument raises `ArgumentError` (E-39), and a runtime-setup, ABI, or profile-floor failure raises `Kobako::SetupError` (E-40..E-42, E-49). The module compile may be amortised across processes by an owner-only on-disk cache whose entries carry exactly the trust of the Guest Binary file; the cache is unobservable beyond construction latency. Each cap defines a per-invocation bound: `timeout` is absolute wall-clock time from the invocation entry (`#eval` / `#run`), expiring at `entry_time + timeout` and enforced at guest wasm safepoints — no trap fires while host code runs, yet wall-clock time a Service callback consumes counts against the deadline. `memory_limit` bounds the cumulative `memory.grow` delta past the linear-memory size observed at invocation entry, so the Guest Binary's initial allocation and prior invocations' watermark sit outside the budget (E-20). `stdout_limit` / `stderr_limit` bound per-channel output capture (B-04). |

---

## B-02 — Invoke `#eval(code)` from a fresh Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with zero prior invocations (no `#eval` and no `#run` call). Zero or more Members have been bound. Zero or more snippets have been preloaded (B-32). The stdout and stderr buffers are empty. |
| **Operation** | `sandbox.eval(code)` where `code` is a String of mruby source. |
| **Result / Final State** | The Catalog::Handles counter is reset and no Handles from any prior invocation are reachable. Service bindings registered on this Sandbox remain active. Preloaded snippets (B-32) replay in insertion order before `code` executes; each snippet contributes its top-level side effects to the invocation's canonical boot state (B-49). `code` then loads with backtrace filename `(eval)`. `#eval` blocks until execution completes, up to the configured `timeout`. On success, `#eval` returns a single deserialized Ruby value — the last mruby expression of `code`, with the exact value semantics refined in B-06. The stdout and stderr buffers contain any output written during execution, bounded by `stdout_limit` / `stderr_limit` (B-04). Per-invocation cap exhaustion surfaces as `Kobako::TimeoutError` (wall-clock `timeout` exceeded; E-19) or `Kobako::MemoryLimitError` (per-invocation `memory.grow` delta exceeds `memory_limit`; E-20), both subclasses of `Kobako::TrapError`. If `code` is `nil`, not a String, or fails compilation, `#eval` raises `Kobako::SandboxError`. This first invocation (`#eval` or `#run`) seals the snippet table and Service registration (B-33). |

---

## B-03 — Invoke `#eval` or `#run` on a Sandbox that has already invoked

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance that has completed one or more prior invocations (any combination of `#eval` and `#run`). Members bound before the first invocation remain registered. Snippets preloaded before the first invocation remain registered. |
| **Operation** | `sandbox.eval(code)` or `sandbox.run(target, *args, **kwargs)` — any invocation after the first. |
| **Result / Final State** | Each invocation executes in a fully isolated context, independent of all prior invocations. All capability state (Handles issued in prior invocations) is fully discarded before the new invocation begins. All Service bindings and all preloaded snippets remain active across invocations and are visible to the new invocation. `#eval` returns the last expression of its source; `#run` returns the entrypoint's `#call` return value (B-31). The stdout and stderr buffers are cleared at the start of this invocation and contain only output from this invocation; the per-channel truncation predicates (B-04) reset together with the buffers. Per-invocation cap enforcement (B-02 Result) applies identically to every invocation, regardless of verb. This isolation is unconditional — it holds whether the previous invocation returned a value or raised an error, uniformly across `#eval` / `#run` boundaries (stale-Handle presentation is covered by B-18). |

---

## B-04 — Read `#stdout` / `#stderr` after an invocation returns

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which `#eval` or `#run` has been called and has returned (either with a value or by raising an error). |
| **Operation** | `sandbox.stdout`, `sandbox.stderr`, `sandbox.stdout_truncated?`, or `sandbox.stderr_truncated?` — any combination, any order, any number of times. |
| **Result / Final State** | Each byte reader returns the content (as a UTF-8 String) the guest wrote to its respective output channel during the most recent invocation, up to the configured `stdout_limit` / `stderr_limit`. The buffers do not change between successive reads. The content contains no kobako protocol bytes and no truncation sentinels. When a channel's cap was reached, the host buffer ends at the cap boundary and subsequent guest writes on that channel fail or are dropped — the guest may rescue the failure or ignore it, but no further bytes reach the buffer; this does not cause the invocation to raise an error. Each truncation predicate returns `true` iff its channel hit its cap during the most recent invocation, otherwise `false`. The per-channel caps are set at construction (B-01). The buffers and predicates are populated on every invocation outcome — value return or any raised error class: after an invocation that raised `Kobako::TrapError` (including E-19 / E-20), each buffer holds the bytes the guest wrote to its channel before the trap fired, up to that channel's cap. Buffers and predicates remain readable after any invocation and reset at the start of the next one (B-03). |

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
| **Result / Final State** | When the guest completes without raising `Kobako::TrapError`, `#eval` returns the deserialized Ruby value of the last mruby expression of `code`. If the last expression evaluates to `nil` (including a `code` with no explicit return expression), `#eval` returns Ruby `nil`. If the last expression is, or contains, a Capability Handle the guest received earlier in this invocation, that Handle is restored to its original host object per B-37. If the last expression produces an object that has no wire representation and is not a Capability Handle, `#eval` raises `Kobako::SandboxError`. Exactly one value is returned per `#eval` call; there is no mechanism to return multiple values or to stream. |

---

## B-35 — Read `#usage` for per-last-invocation resource accounting

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance, either fresh (no prior invocation) or post-invocation. |
| **Operation** | `sandbox.usage` — any number of times, any order, before or after `#eval` / `#run`. |
| **Result / Final State** | Returns a `Kobako::Usage` value object exposing two readers: `wall_time` (Float seconds the guest export call spent inside the wasmtime engine during the most recent invocation) and `memory_peak` (Integer bytes, the high-water mark of the per-invocation `memory.grow` delta past the linear-memory size observed at invocation entry). Both readers reflect the most recent invocation only — the next `#eval` / `#run` overwrites them at the start of that invocation. Before any invocation, `#usage` returns the pre-invocation sentinel `Kobako::Usage::EMPTY` (`wall_time` = `0.0`, `memory_peak` = `0`). `wall_time` brackets the guest export call — it opens when the per-invocation caps are armed and closes when wasmtime returns control to the host, so it includes time spent in host Service callbacks (consistent with B-01's `timeout` accounting) and excludes the post-export outcome fetch, its decode, and the capture readout. `memory_peak` shares its baseline accounting with `memory_limit` (B-01, E-20). Both readers are populated on every invocation outcome — value return or any raised error class — so the Host App can read `#usage` after a rescue; on `MemoryLimitError`, `memory_peak` reports the largest delta the limiter accepted, never exceeding `memory_limit`. |
