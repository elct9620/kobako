# Lifecycle — construction, `#eval`, output, usage

Sandbox construction, the `#eval` one-shot invocation, output capture, and per-invocation usage. The governing summary lives in [`SPEC.md`](../../SPEC.md)
§ Behavior; this file is the per-anchor reference. `B-xx` anchors are global
and append-only across the corpus (N-8).

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

## B-35 — Read `#usage` for per-last-invocation resource accounting

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance, either fresh (no prior invocation) or post-invocation. |
| **Operation** | `sandbox.usage` — any number of times, any order, before or after `#eval` / `#run`. |
| **Result / Final State** | Returns a `Kobako::Usage` value object exposing two readers: `wall_time` (Float seconds the guest export call spent inside the wasmtime engine during the most recent invocation) and `memory_peak` (Integer bytes, the high-water mark of the per-invocation `memory.grow` delta past the linear-memory size observed at invocation entry). Both readers reflect the most recent invocation only — the next `#eval` / `#run` overwrites them at the start of that invocation. Before any invocation, `#usage` returns the pre-invocation sentinel `Kobako::Usage::EMPTY` (`wall_time` = `0.0`, `memory_peak` = `0`). |
| **Notes** | `wall_time` is measured on the host around the guest export call (`__kobako_eval` / `__kobako_run`): the bracket opens when the per-invocation caps are armed and closes when wasmtime returns control to the host, so it includes time spent in host Service callbacks (consistent with the `timeout` accounting in B-01 Notes) and excludes the post-export `OUTCOME_BUFFER` fetch, its msgpack decode, and the stdout / stderr capture readout. `memory_peak` shares its baseline accounting with `memory_limit` (B-01, E-20). Both readers are populated on every invocation outcome — value return or any raised error class — so the Host App can read `#usage` after a rescue; on `MemoryLimitError`, `memory_peak` reports the largest delta the limiter accepted, never exceeding `memory_limit`. |
