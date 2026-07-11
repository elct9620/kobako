# Error scenarios

Every Sandbox invocation terminates in exactly one of four outcomes; this file
details E-01..E-50 and the two-step attribution decision. The governing summary
lives in [`SPEC.md`](../../SPEC.md) § Behavior. `E-xx` anchors are global and
append-only across the corpus (N-8).

## Error Scenarios

Every Sandbox invocation (`#eval` or `#run`) terminates in exactly one of four outcomes: a return value, `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError`. Attribution is determined by a two-step decision applied after the invocation export returns (`__kobako_eval` for `#eval`, `__kobako_run` for `#run`):

**Step 1 — Trap detection (highest priority).**
If the Wasm engine reports a trap (e.g., wasmtime raises a native trap exception), the outcome is `Kobako::TrapError` or one of its named subclasses regardless of any other state. No outcome bytes are inspected. The trap kind determines the raised class: wall-clock timeout traps raise `Kobako::TimeoutError` (E-19), linear-memory-cap traps raise `Kobako::MemoryLimitError` (E-20), and all other engine or wire-violation traps raise the base `Kobako::TrapError` (E-01..E-03).

**Step 2 — Outcome envelope tag (non-trap outcomes only).**
If no trap occurred, the Host Gem reads the outcome bytes produced by `__kobako_take_outcome` and dispatches on the first-byte tag:

| First-byte tag | Outcome bytes state | Raised class |
|---------------|---------------------|--------------|
| — | Zero-length (`len == 0`) | `Kobako::TrapError` — wire violation fallback (a *wire violation* is any guest binary output that does not conform to the wire codec; → [`docs/wire-codec.md`](../wire-codec.md) § Type Mapping) |
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

**Cross-references:** E-02 and E-03 are the wire-violation fallback paths invoked by any malformed Guest Binary output. B-21 (Handle counter exhaustion) raises `Kobako::HandleExhaustedError` (a `SandboxError` subclass), not `TrapError`. E-19 fires only at guest wasm safepoints — a Service callback running on the host cannot itself trigger E-19 — but the wall-clock time consumed by host callbacks counts against the `timeout` budget (B-01 Notes).

---

### `Kobako::SandboxError`

Raised when the guest execution environment ran to completion but the overall execution failed due to a protocol fault, a mruby runtime error, or a Host Gem–side wire decode failure. The guest Wasm instance is retired normally; the sandbox infrastructure itself is intact.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-04 | Guest mruby script raises an uncaught exception (e.g., `RuntimeError`, `NoMethodError`) that reaches the top level of the invocation export (`__kobako_eval` or `__kobako_run`) — including a raise inside a guest callback a capability gem invokes (B-51) | B-02, B-03 — script execution; B-51 — capability-gem callback raise |
| E-05 | The guest fails to compile the source supplied to `#eval` before any execution begins | B-02 — fresh invocation |
| E-06 | The invocation's return value has no wire representation — the `#eval` last expression or the `#run` entrypoint's `#call` return is a raw mruby `Object` with no MessagePack encoding, or nests beyond the maximum encodable depth (a reference cycle necessarily does; → [`docs/wire-codec.md`](../wire-codec.md) § Structural Nesting Depth); outcome tag `0x01` is present but the value field fails to decode | B-06, B-31 — return value semantics |
| E-07 | Handle issuance for the returned object fails because the per-invocation Handle counter has reached `0x7fff_ffff` (2³¹ − 1); raised as the `Kobako::HandleExhaustedError` subclass | B-21 — Handle counter exhaustion |
| E-08 | Outcome tag is `0x02` (panic) and the panic envelope is malformed or missing required fields | Step 2 attribution table |
| E-09 | Outcome tag is `0x01` (result) and the result envelope is malformed or fails MessagePack parse | Step 2 attribution; B-06 fallback |
| E-10 | Guest presents an invalid wire payload as a dispatch argument (e.g., a raw integer where a Capability Handle ext type `0x01` is required) | B-20 — guest cannot forge Handles |
| E-21 | Guest block uses `return val` while its enclosing method is still on the guest call stack (non-lambda, non-orphan Proc); the unwind crosses the host yield boundary, which is unrepresentable on the wire | B-24 — yield round-trip |
| E-22 | Guest block returns a value that has no MessagePack wire representation per [`docs/wire-codec.md`](../wire-codec.md) § Type Mapping, or that nests beyond the maximum encodable depth (a reference cycle necessarily does; § Structural Nesting Depth) | B-24 — yield round-trip |
| E-23 | Host Service method invokes its Yielder after the originating dispatch frame has returned (e.g., the Service stored the block via `&block` and called it from a later dispatch or post-dispatch host code) | B-23 — Yielder scope |
| E-50 | A guest→host payload carries an ext 0x02 Fault envelope — in a dispatch Request, a YieldResponse value, a Result envelope value, or a Panic envelope — violating the fault envelope's sole legal wire position (the Response `status=1` fault field, → [`docs/wire-codec.md`](../wire-codec.md) § ext 0x02). The Result / Panic envelope paths raise `Kobako::Transport::Error`; the Request path rejects the dispatch through the malformed-payload channel (`type="runtime"`), and the YieldResponse path raises at the Service yield site — both surfacing as `Kobako::ServiceError` when the script leaves the failure unrescued | B-06 — return value; B-12 — dispatch; B-24 — yield round-trip |

---

### `Kobako::ServiceError`

Raised when the guest execution environment ran to completion, the mruby script itself did not crash, but a Service capability call reported an application-level failure. The error originates in host Service code or in the capability routing layer, not in mruby script logic or the Wasm engine.

`ServiceError` is raised when a panic envelope with `origin == "service"` reaches the host — meaning the mruby script executed a Service dispatch that failed and the failure was not rescued within the script.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-11 | A bound Service method raises a Ruby exception during dispatch; the exception propagates through the dispatch response as `status=1`, error `type="runtime"`, and the mruby script does not rescue it | B-12 — Transport dispatch |
| E-12 | The dispatch `target` path (e.g., `"MyService::KV"`) does not match any registered Service; error `type="undefined"` returned; mruby script does not rescue it | B-08, B-12 — undefined target |
| E-13 | The dispatch `target` is a Handle ID that does not exist in the current invocation (stale Handle from a prior invocation presented as target in a new invocation); error `type="undefined"` | B-18 — stale Handle cross-invocation |
| E-15 | Service method receives arguments that fail the host-side parameter binding (e.g., unknown keyword); error `type="argument"` returned; mruby guest does not rescue it. Passing keyword arguments to a method whose signature accepts no keyword arguments is treated as a parameter binding failure (`type="argument"`, E-15), not a Ruby runtime exception (E-11). | B-12 — Transport dispatch |
| E-43 | The dispatch method resolves, on the target, to Ruby's ambient reflection / eval surface — owner in a core meta module (`BasicObject` / `Kernel` / `Object` / `Module` / `Class`) or a callable gadget type (`Proc` / `Method` / `UnboundMethod` / `Binding`) outside the callable allowlist; error `type="undefined"` returned; mruby script does not rescue it | B-42 — reflection rejection |
| E-44 | A bound Service method returns a `Binding`, `Method`, or `UnboundMethod` — directly, or extracted by the guest from a returned container Handle; the host refuses to mint a Capability Handle and the dispatch reports `type="runtime"`; the mruby script does not rescue it | B-43 — reflective gadget not wire-representable |
| E-48 | The dispatch method name is rejected by the target's own narrowing predicate — the bound object defines `respond_to_guest?` and it answers falsy for the name (B-50); error `type="undefined"` returned; mruby script does not rescue it | B-50 — guest-surface narrowing |

A guest attempting to forge a Handle from a bare integer is rejected by the guest-side wire decoder before any dispatch reaches the host; that path raises `Kobako::SandboxError` (E-10), not `ServiceError` (B-20).

When the guest wraps a Service call in `begin/rescue`, the dispatch failure is handled within the guest; no `ServiceError` reaches the host and the invocation returns normally. `Kobako::ServiceError` is raised to the Host App only when a Service failure is unrescued at the top level of the guest execution context.

E-14 is a retired anchor — permanently reserved and never reassigned (N-8).

---

### `Kobako::SetupError`

Raised by `Kobako::Sandbox.new` when the wasm runtime cannot be constructed from the configured `wasm_path` (B-01), the Guest Binary fails the ABI version check (B-40), or the runtime's declared isolation profile falls below the requested floor (B-54) — before any invocation entry point runs. Construction is a setup verb, not an invocation: `SetupError` is therefore not one of the four invocation outcomes and does not pass through the two-step attribution decision, mirroring the E-16 / E-45 setup-time treatment. Because no Sandbox instance is produced, the `TrapError` "discard and recreate" recovery contract does not apply — a `SetupError` reflects a deterministic artifact or environment fault, and retrying `Sandbox.new` against the same `wasm_path` fails identically until the underlying cause is fixed.

`Kobako::ModuleNotBuiltError` is the named subclass for the common, actionable case: the Guest Binary artifact has not been built yet. A Host App that only needs "the Sandbox could not be set up" can rescue `Kobako::SetupError`; one that wants to special-case the unbuilt-artifact state can rescue `Kobako::ModuleNotBuiltError` first.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-39 | `Sandbox.new` option argument is invalid: `timeout` is non-Numeric, non-positive, or non-finite, `memory_limit` is not a positive Integer, or `profile` is not a ladder value — `nil` included; the weakest posture is requested explicitly as `:permissive` (B-54) | host pre-flight (`SandboxOptions`, before any engine work) | `ArgumentError` |
| E-40 | The Guest Binary artifact is absent at the resolved `wasm_path` — the common state on a fresh clone before `rake compile` | construction: artifact lookup | `Kobako::ModuleNotBuiltError` |
| E-41 | The Guest Binary artifact is present but the wasm runtime cannot be constructed from it: the file cannot be read, its bytes are not a valid Wasm module, or engine / linker / instantiation setup fails | construction: read / compile / instantiate | `Kobako::SetupError` |
| E-42 | The Guest Binary does not export `__kobako_abi_version`, or the export's reported value differs from the ABI version the Host Gem implements (→ [`docs/wire-codec.md`](../wire-codec.md) § ABI Version) | construction: ABI version probe (B-40) | `Kobako::SetupError` |
| E-47 | `Pool.new` argument is invalid: `slots` is not a positive Integer, or `checkout_timeout` is non-Numeric, non-positive, or non-finite (`nil` is valid and waits indefinitely) | host pre-flight (`Pool.new`, before any engine work) | `ArgumentError` |
| E-49 | The runtime's declared isolation profile is below the posture requested via `profile:` (B-54) | construction: profile floor check | `Kobako::SetupError` |

E-42's actionable remedy is rebuilding the Guest Binary against the Host Gem's ABI version.

---

### `Kobako::PoolTimeoutError`

Raised by `Kobako::Pool#with` when the checkout wait exceeds the configured `checkout_timeout` (B-47). Checkout is a pool verb, not an invocation: `PoolTimeoutError` is not one of the four invocation outcomes and does not pass through the two-step attribution decision. No Sandbox state is touched — every pooled Sandbox is exactly as the other holders left it, and retrying `#with` succeeds as soon as a holder returns its Sandbox.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-46 | `Pool#with` waited `checkout_timeout` seconds while all `slots` Sandboxes were held by other callers (B-47) | pool checkout, before any Sandbox is touched | `Kobako::PoolTimeoutError` |

---

### Registration errors (`bind`)

These error scenarios cover Service binding (B-08, B-09, B-11) and the sealing rule (B-33). All are Host App programming errors detected at setup time, before or between guest executions; they raise `ArgumentError` synchronously and do not engage the attribution pipeline.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-16 | `sandbox.bind(path, obj)` with a `path` segment not matching the `/\A[A-Z]\w*\z/` constant pattern (B-08) | host pre-flight | `ArgumentError` |
| E-45 | `sandbox.bind` after the first invocation (`#eval` or `#run`) has sealed Service registration (B-08, B-33); the existing bindings and the Frame 1 preamble of subsequent invocations are unchanged | host pre-flight | `ArgumentError` |

E-17 is a retired anchor — permanently reserved and never reassigned (N-8). E-18 is a retired anchor — permanently reserved and never reassigned (N-8).

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

These error scenarios are specific to the `#preload` setup verb (B-32) — covering both the `code:` source form and the `binary:` bytecode form — and the sealing rule (B-33). Host pre-flight API-misuse cases raise `ArgumentError` synchronously. Content failures originating in user-supplied snippets surface as `Kobako::SandboxError`, with the `Kobako::BytecodeError` subclass reserved for `binary:` form structural failures. A failure raised by executing snippet code carries backtrace attribution under the snippet's filename (always for `code:`; for `binary:` only when the bytecode carries `debug_info`); a snippet that fails to load at all (E-32, E-37, E-38) runs no snippet code and raises with an empty backtrace.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-32 | `#preload(code:)` source fails mruby compilation when a subsequent invocation replays the snippet | guest replay (first invocation) | `Kobako::SandboxError` (mruby's generic syntax-error message; compilation runs no snippet code, so the backtrace is empty) |
| E-33 | `#preload(code:)` `name:` matches the name of a `code:` form snippet already registered on the Sandbox | host pre-flight | `ArgumentError` |
| E-34 | `#preload(code:)` `name:` does not match `/\A[A-Z]\w*\z/` | host pre-flight | `ArgumentError` |
| E-35 | `#preload` is called after the first invocation (`#eval` or `#run`) — the snippet table is sealed per B-33 | host pre-flight | `ArgumentError` |
| E-36 | A preloaded snippet's top-level expression raises during replay inside a subsequent invocation. Covers both `#preload(code:)` and `#preload(binary:)` forms — `binary:` form structural failures (E-37 / E-38) are separate. | guest static load | `Kobako::SandboxError` (backtrace attributed to `(snippet:Name)` when the snippet carries a filename) |
| E-37 | `#preload(binary:)` bytecode's RITE version does not match the version the guest mruby was built against | guest replay (first invocation) | `Kobako::BytecodeError` |
| E-38 | `#preload(binary:)` bytecode body is corrupt or malformed and fails to load during snippet replay | guest replay (first invocation) | `Kobako::BytecodeError` |

E-33 is scoped to `code:` form snippets: duplicate `code:` form names would produce ambiguous `(snippet:Name):line` attribution in backtraces, so two `code:` snippets with the same `name:` are never permitted on a single Sandbox. The host does not extract names from `binary:` form bytecode, so cross-form name collisions are not detected at preload — users who need class reopening across multiple bodies must concatenate the sources under one snippet or use distinct names per layer.

The backtrace filename `(snippet:Name)` is the locator that ties a replay failure back to the specific `#preload` call; stripped `binary:` payloads omit the frame per B-32.

Subsequent invocations on the same Sandbox replay the same bytecode into the canonical boot state (B-49) and raise the same `Kobako::BytecodeError` deterministically (B-33 seals the table). Bytecode that loads structurally but lacks `debug_info` is not a structural failure — see B-32 for its observable effect on backtrace attribution.
