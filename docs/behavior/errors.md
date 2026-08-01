# Error scenarios

Every Sandbox invocation terminates in exactly one of four outcomes; this file
details every `E-xx` scenario and the two-step attribution decision. The governing summary
lives in [`SPEC.md`](../../SPEC.md) § Behavior. `E-xx` anchors are global and
append-only across the corpus (N-8).

## Error Scenarios

Every Sandbox invocation (`#eval` or `#run`) terminates in exactly one of four outcomes: a return value, `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError`. Attribution is determined by a two-step decision applied after the invocation export returns (`__kobako_eval` for `#eval`, `__kobako_run` for `#run`):

**Step 1 — Trap detection (highest priority).**
If the Wasm engine reports a trap (e.g., wasmtime raises a native trap exception), the outcome is `Kobako::TrapError` or one of its named subclasses regardless of any other state. No outcome bytes are inspected. The trap kind determines the raised class: wall-clock timeout traps raise `Kobako::TimeoutError` (E-19), linear-memory-cap traps raise `Kobako::MemoryLimitError` (E-20), and all other engine or wire-violation traps raise the base `Kobako::TrapError` (E-01..E-03).

**Step 2 — Outcome arm (non-trap outcomes only).**
If no trap occurred, the Host Gem frames the outcome bytes produced by `__kobako_take_outcome` and dispatches on the arm the core envelope names:

| Outcome arm | State | Raised class |
|-------------|-------|--------------|
| — | The bytes are not an Outcome the envelope can frame, an absent outcome included | `Kobako::TrapError` — wire violation fallback (a *wire violation* is any guest binary output that does not conform to the wire codec; → [`docs/wire/payload-msgpack.md`](../wire/payload-msgpack.md) § Type Mapping) |
| ok | The value decodes | Return value (no error raised) |
| ok | The payload codec cannot read the value | `Kobako::SandboxError` |
| Panic | `origin == "service"` | `Kobako::ServiceError` |
| Panic | any other `origin` | `Kobako::SandboxError` |

Attribution reads the core envelope alone, and the codec takes no part in it: every Panic field — `origin`, class, message, backtrace, and the `available` names a correction can be offered from — is typed at that layer, so a Panic carries nothing the codec reads.

`stdout` and `stderr` bytes do not participate in attribution dispatch. They are always available via the run's `Execution` — the one a raised error carries on `#execution` — after a rescue, including after error-raising runs.

---

## Dispatch failure attribution

A guest→host dispatch that does not return a value answers on the Reply's fault arm (→ [`docs/wire-contract.md`](../wire-contract.md) § Fault). The category the Fault carries decides what the guest raises, and — when the guest leaves it unrescued — what the Host App rescues. One table fixes both, so neither side has to re-derive which failure belongs to whom.

The **whose failure** column is what the category is chosen by. It answers who has to change something for the same call to succeed, which is the question a Host App triaging a failure and a guest deciding whether to retry are both asking.

| Trigger | Whose failure | Fault category | Guest raises | Host App rescues | Anchor |
|---|---|---|---|---|---|
| The `target` path holds no Service, or holds an unresolved fillable | Undisclosed — the three `undefined` causes stay indistinguishable, so an opaque target reveals nothing about what it defines | `undefined` | `Kobako::NoServiceError` | `Kobako::NoServiceError` | E-12, B-62 |
| The `target` Handle id is not live in this invocation | Undisclosed, as above | `undefined` | `Kobako::NoServiceError` | `Kobako::NoServiceError` | E-13, B-65 |
| The method is absent, resolves to ambient reflection surface, or the target's own predicate denies it | Undisclosed, as above | `undefined` | `Kobako::NoServiceError` | `Kobako::NoServiceError` | E-43, E-48 |
| The arguments do not fit the Service method | The guest's call site — or the Service's own body, which this category does not tell apart | `argument` | `Kobako::ServiceArgumentError` | `Kobako::ServiceArgumentError` | E-15 |
| The Service method raised | The bound object, or what it calls | `runtime` | `Kobako::ServiceError` | `Kobako::ServiceError` | E-11 |
| The Service method returned a reflective gadget the host refuses to mint a Handle for | The Host App's Service | `runtime` | `Kobako::ServiceError` | `Kobako::ServiceError` | E-44 |
| A Yielder was invoked after its dispatch frame returned | The Host App's Service | `runtime` | `Kobako::ServiceError` | `Kobako::ServiceError` | E-23 |
| The request is one the host's codec cannot read | Whoever wrote the guest's payload codec — the Service never ran | `internal` | `Kobako::Transport::Error` | `Kobako::Transport::Error` | E-10 |
| The Handle table is exhausted, so the answer cannot be issued one | Neither side's — a per-invocation resource ran out | `internal` | `Kobako::Transport::Error` | `Kobako::Transport::Error` | E-07 |
| The guest's own block raised and the Service did not rescue it | The guest's block | `block` | the exception the block raised, continued | `Kobako::SandboxError` carrying that class on `klass` | E-04, B-24 |
| The guest's own block produced a value it cannot represent | The guest's block | `block` | `Kobako::ServiceError` naming the refused class in its message — the block raised nothing, so there is no exception to continue | `Kobako::ServiceError` | E-21, E-22 |

Four rules hold the table together:

- **The category never claims more than the host knows.** `undefined` is deliberately one category for three causes; `internal` says a Service outcome does not exist rather than that a Service failed; `block` says the failure came back from where it was sent.
- **A guest-side subclass narrows, never crosses.** `NoServiceError` and `ServiceArgumentError` are `Kobako::ServiceError` subclasses on both sides, so one `rescue Kobako::ServiceError` still catches every Service failure. A Panic's class name may only select a subclass of the class its `origin` already chose (Step 2 above), so what a guest calls its exception cannot move the failure to another layer.
- **A raised exception's class travels in the message; a refusal's does not.** An exception a Service raises crosses as `<class>: <message>` (B-12) — the shape a Host App is told to keep secrets out of — and so does a guest block's, because the guest may have no object left to continue. A refusal kobako itself produced answers under its own wording, so a host-side internal class is never presented as something a Service raised.
- **`argument` is the one category named by the exception rather than by who failed.** It reports the `ArgumentError` the dispatch boundary caught, which a Service raising one from inside its own body produces as readily as a genuine binding mismatch.

---

### `Kobako::TrapError` and its subclasses

Raised when the Wasm execution engine crashes, when the wire layer detects a structural violation that signals a corrupted guest execution environment, or when a configured per-invocation cap is exceeded. The base class `Kobako::TrapError` covers engine and wire-violation traps; the named subclasses `Kobako::TimeoutError` and `Kobako::MemoryLimitError` cover the configured-cap cases. After any TrapError (base class or subclass), the Sandbox is considered unrecoverable; Host Apps should discard and recreate it before the next invocation.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-01 | Wasm engine trap: `unreachable` instruction, stack overflow, or import signature mismatch | Wasm engine reports a native trap; Step 1 fires | `Kobako::TrapError` |
| E-02 | Guest exited without writing any outcome bytes (`len == 0`) | Step 2: zero-length outcome bytes; wire violation fallback | `Kobako::TrapError` |
| E-03 | The outcome bytes are not an Outcome the core envelope can frame — an unrecognized arm tag, or a Panic arm the layout cannot read | Step 2: unframeable outcome; wire violation fallback | `Kobako::TrapError` |
| E-19 | Absolute wall-clock time since invocation entry (`Sandbox#eval` or `Sandbox#run`) reached the configured `timeout` and a guest wasm safepoint was hit thereafter (B-01) | Wasm engine reports a wall-clock interrupt at the first guest wasm safepoint after the absolute deadline; Step 1 fires | `Kobako::TimeoutError` |
| E-20 | Cumulative guest `memory.grow` since invocation entry would push past the configured `memory_limit` (B-01) | Wasm engine reports a memory-cap trap; Step 1 fires | `Kobako::MemoryLimitError` |

**Cross-references:** E-02 and E-03 are the wire-violation fallback paths invoked by any malformed Guest Binary output. B-21 (Handle counter exhaustion) raises `Kobako::HandleExhaustedError` (a `SandboxError` subclass), not `TrapError`. E-19 fires only at guest wasm safepoints — a Service callback running on the host cannot itself trigger E-19 — but the wall-clock time consumed by host callbacks counts against the `timeout` budget (B-01 Notes).

---

### `Kobako::SandboxError`

Raised when the guest execution environment ran to completion but the overall execution failed due to a protocol fault, a mruby runtime error, or a Host Gem–side wire decode failure. The guest Wasm instance is retired normally; the sandbox infrastructure itself is intact.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-04 | Guest mruby script raises an uncaught exception (e.g., `RuntimeError`, `NoMethodError`) that reaches the top level of the invocation export (`__kobako_eval` or `__kobako_run`) — including a raise inside a guest callback a capability gem invokes (B-51), and a raise inside a block the guest supplied to a Service that did not rescue it, which continues in the guest frame that raised it (B-24) | B-02, B-03 — script execution; B-51 — capability-gem callback raise; B-24 — yield round-trip |
| E-05 | The guest fails to compile the source supplied to `#eval` before any execution begins | B-02 — fresh invocation |
| E-06 | The invocation's return value has no wire representation — the `#eval` last expression or the `#run` entrypoint's `#call` return is a raw mruby `Object` with no MessagePack encoding, nests beyond the maximum encodable depth (a reference cycle necessarily does; → [`docs/wire/payload-msgpack.md`](../wire/payload-msgpack.md) § Structural Nesting Depth), or is a `Symbol` whose name is not UTF-8 (→ [`docs/wire/payload-msgpack.md`](../wire/payload-msgpack.md) § Text and Bytes); the ok arm is present but its value fails to decode | B-06, B-31 — return value semantics |
| E-07 | Handle issuance for the returned object fails because the per-invocation Handle counter has reached `0x7fff_ffff` (2³¹ − 1). On the `#run` auto-wrap path (B-34) the host raises `Kobako::HandleExhaustedError` directly; on the dispatch path the exhaustion answers `type="internal"` and reaches an unrescuing guest as `Kobako::Transport::Error` — the answer never existed, so no Service outcome is being reported | B-21 — Handle counter exhaustion |
| E-09 | The Outcome's ok arm carries a value the payload codec cannot read | Step 2 attribution; B-06 fallback |
| E-10 | The Call payload is one the host's codec cannot read — a kwargs key that is not a Symbol, nesting past the codec's depth bound, or a Capability Handle frame outside the shape [`docs/wire/payload-msgpack.md`](../wire/payload-msgpack.md) § Ext Types → ext 0x01 admits. This is the **malformed-payload channel**: the host answers `type="internal"` on the fault arm rather than trapping, so a payload no schema can read reaches the guest as an ordinary transport failure it may rescue. Unrescued it raises `Kobako::Transport::Error`, a `SandboxError` subclass — the request never became a call, so nothing about a Service failed. The bundled guest emits none of these; the channel exists for a guest whose payload codec kobako does not define | B-12 — Transport dispatch |
| E-55 | Guest passes a dispatch argument, kwargs value, or keyword name with no wire representation — a value outside the 11-entry wire type set, a collection nesting beyond the maximum encodable depth (a reference cycle necessarily does; → [`docs/wire/payload-msgpack.md`](../wire/payload-msgpack.md) § Structural Nesting Depth), or a name that is not UTF-8 — a `Symbol` value's, or a keyword's (→ [`docs/wire/payload-msgpack.md`](../wire/payload-msgpack.md) § Text and Bytes). The guest rejects it at the dispatch call site rather than coercing it to an `Object#to_s` string, uniform with the return-value (E-06) and yield-block (E-22) rejections | B-12 — dispatch argument conversion |

---

### `Kobako::ServiceError` and its subclasses

Raised when the guest execution environment ran to completion, the mruby script itself did not crash, but a Service capability call reported an application-level failure. The error originates in host Service code or in the capability routing layer, not in mruby script logic or the Wasm engine.

`ServiceError` is raised when a panic envelope with `origin == "service"` reaches the host — meaning the mruby script executed a Service dispatch that failed and the failure was not rescued within the script. The base class covers a Service that ran and raised; `Kobako::NoServiceError` and `Kobako::ServiceArgumentError` cover the calls that never reached one, so a Host App routes them apart with `rescue` rather than by reading the message. § Dispatch failure attribution above is the whole mapping in one table.

| # | Trigger | Raised class | Behavior cross-reference |
|---|---------|--------------|--------------------------|
| E-11 | A bound Service method raises a Ruby exception during dispatch; the exception propagates through the dispatch Reply's fault body (`tag=1`) as fault `type="runtime"`, and the mruby script does not rescue it | `Kobako::ServiceError` | B-12 — Transport dispatch |
| E-12 | The dispatch `target` path (e.g., `"MyService::KV"`) does not match any registered Service; fault `type="undefined"` returned; mruby script does not rescue it | `Kobako::NoServiceError` | B-08, B-12 — undefined target |
| E-13 | The dispatch `target` is a Handle ID that does not exist in the current invocation (stale Handle from a prior invocation presented as target in a new invocation); fault `type="undefined"` | `Kobako::NoServiceError` | B-18 — stale Handle cross-invocation |
| E-15 | Service method receives arguments that fail the host-side parameter binding (e.g., unknown keyword); fault `type="argument"` returned; mruby guest does not rescue it. Passing keyword arguments to a method whose signature accepts no keyword arguments is treated as a parameter binding failure (`type="argument"`, E-15), not a Ruby runtime exception (E-11). | `Kobako::ServiceArgumentError` | B-12 — Transport dispatch |
| E-43 | The dispatch method resolves, on the target, to Ruby's ambient reflection / eval surface — owner in a core meta module (`BasicObject` / `Kernel` / `Object` / `Module` / `Class`) or a callable gadget type (`Proc` / `Method` / `UnboundMethod` / `Binding`) outside the callable allowlist; fault `type="undefined"` returned; mruby script does not rescue it | `Kobako::NoServiceError` | B-42 — reflection rejection |
| E-44 | A bound Service method returns a `Binding`, `Method`, or `UnboundMethod` — directly, or extracted by the guest from a returned container Handle; the host refuses to mint a Capability Handle and the dispatch reports `type="runtime"`; the mruby script does not rescue it | `Kobako::ServiceError` | B-43 — reflective gadget not wire-representable |
| E-48 | The dispatch method name is rejected by the target's own narrowing predicate — the bound object defines `respond_to_guest?` and it answers falsy for the name (B-50); fault `type="undefined"` returned; mruby script does not rescue it | `Kobako::NoServiceError` | B-50 — guest-surface narrowing |
| E-21 | Guest block uses `return val` while its enclosing method is still on the guest call stack (non-lambda, non-orphan Proc); the unwind crosses the host yield boundary, which is unrepresentable on the wire. The block raised nothing, so the guest has no exception to continue and the fault names the class in its message | `Kobako::ServiceError` | B-24 — yield round-trip |
| E-22 | Guest block returns a value that has no MessagePack wire representation per [`docs/wire/payload-msgpack.md`](../wire/payload-msgpack.md) § Type Mapping, that nests beyond the maximum encodable depth (a reference cycle necessarily does; § Structural Nesting Depth), or that is a `Symbol` whose name is not UTF-8 (§ Text and Bytes). As E-21, the block refused a value rather than raising | `Kobako::ServiceError` | B-24 — yield round-trip |
| E-23 | Host Service method invokes its Yielder after the originating dispatch frame has returned (e.g., the Service stored the block via `&block` and called it from a later dispatch or post-dispatch host code) | `Kobako::ServiceError` | B-23 — Yielder scope |

A guest presenting an arbitrary integer as a Call `target` reaches the host, where the invocation's `Catalog::Handles` membership answers it with `type="undefined"` (E-13, B-65) — the `Kobako::NoServiceError` path when the script leaves it unrescued.

The `Kobako::ServiceArgumentError` boundary is the exception class the host caught, not which side supplied the mismatch: an `ArgumentError` a Service method raises from inside its own body is reported as an argument failure alongside a genuine binding mismatch. Telling the two apart would mean deciding, at the dispatch boundary, whether the method body ran — which the exception itself does not say.

When the guest wraps a Service call in `begin/rescue`, the dispatch failure is handled within the guest; no `ServiceError` reaches the host and the invocation returns normally. `Kobako::ServiceError` is raised to the Host App only when a Service failure is unrescued at the top level of the guest execution context.

E-08 is a retired anchor — permanently reserved and never reassigned (N-8). A Panic is framed entirely by the core envelope, so there is no codec read on that arm to fail.

E-14 is a retired anchor — permanently reserved and never reassigned (N-8).

E-50 is a retired anchor — permanently reserved and never reassigned (N-8). It guarded the one position a Fault was legal in while a Fault was a payload value; a Fault rides the core envelope's own fault arm now, so no payload can carry one and there is no misplacement left to detect.

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
| E-26 | The Run envelope the command buffer carries does not frame, or its payload does not decode — a shape the payload codec cannot read, or an argument value with no faithful guest representation such as an integer outside the signed 32-bit range (→ [`docs/wire/payload-msgpack.md`](../wire/payload-msgpack.md) § Integer Range). The guest reports the two layers separately, so a framing desync stays distinguishable from a payload the codec could not read | guest entry | `Kobako::SandboxError` |
| E-27 | `#run` target Symbol does not resolve to a defined constant on top-level `Object`; the guest's Panic envelope `available` field carries the top-level constants contributed by preloaded snippets, which the raised error exposes as `#available` alongside the `#name` that was asked for | guest: target Symbol does not name a defined top-level constant | `Kobako::UndefinedEntrypointError` |
| E-28 | `#run` entrypoint constant is defined but does not respond to `#call` | guest: entrypoint constant does not respond to `#call` | `Kobako::SandboxError` |
| E-29 | `#run` `args` or `kwargs` contains a `Kobako::Handle` instance. The Handle constructor is internal to the Host Gem; legitimate Handle production paths (B-14 service return, B-34 host-side auto-wrap) live inside the wire layer and never expose a Handle object to the Host App's call site. Any Handle reaching this position is therefore forged through a non-public path and is rejected | host pre-flight | `ArgumentError` |
| E-30 | `#run` `kwargs` contains a key that is not a Symbol | host pre-flight | `ArgumentError` |
| E-31 | Host's `__kobako_alloc` returns 0 when reserving guest memory for the Run envelope | host pre-call | `Kobako::SandboxError` |
| E-54 | `#run` `args` or `kwargs` nests beyond the maximum encodable depth — a reference cycle necessarily does (→ [`docs/wire/payload-msgpack.md`](../wire/payload-msgpack.md) § Structural Nesting Depth); the host rejects it while encoding the payload rather than recursing without bound | host pre-call | `Kobako::SandboxError` |

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

---

### Extension errors (`#install`)

These error scenarios are specific to the `#install` setup verb (B-55..B-57). Each is a Host App programming error — a malformed Extension, a call after the seal, or an incomplete dependency set — raised as `ArgumentError` synchronously without engaging the attribution pipeline. Because `#install` composes `#preload` and `#bind`, a malformed `name` or `backend.path` surfaces through those verbs' own errors (E-34 for a non-constant snippet name, E-16 for a non-constant bind segment); the anchors below cover the shape checks specific to the Extension contract.

| # | Trigger | Detection point | Raised class |
|---|---------|-----------------|--------------|
| E-51 | `#install` is called after the first invocation (`#eval` or `#run`) — registration is sealed per B-33 | host pre-flight | `ArgumentError` |
| E-52 | An installed Extension names in `depends_on` an Extension that was not installed; detected when the first invocation seals the registries (B-57), before the guest runs, naming the missing dependency | host, at first-invocation seal | `ArgumentError` |
| E-53 | An Extension is malformed for `#install`: its `source` is absent or not a String, or its `backend` is present but does not expose `path`, `object`, and `provider` | host pre-flight | `ArgumentError` |

E-53 covers the Extension-shape checks `#preload` / `#bind` do not: `source` is mandatory (the install/bind boundary — a host object with no guest idiom is bound with `#bind`, not installed), and a present `backend` must expose `path` / `object` / `provider` (duck-typed, like the Extension itself). A malformed `name` reuses E-34 and a malformed `backend.path` reuses E-16, since `#install` routes them through `#preload` and `#bind` respectively. A `provider:` callable that raises during per-invocation resolution is not an install-time shape error — its exception propagates unchanged, specified in B-56.

---

### Unserved payload positions

A payload codec fills the positions it serves and refuses at the rest (→ [`docs/wire-codec.md`](../wire-codec.md) § What a replacement codec must provide). Carrying one value is the floor, so a guest always completes an invocation; every other position is a capability its codec may not offer.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-56 | A guest reaches a payload position its own codec does not serve. The refusal is distinguishable from an unreadable message: nothing was wrong with the bytes or the value, the capability is absent | B-12 — dispatch argument conversion; B-24 — yield round-trip; B-31 — `#run` argument decoding |

The class follows the position, because the position fixes who is listening:

| Position | Raised class | Why |
|----------|--------------|-----|
| Dispatch argument, yield argument, block return value, block `break` value | `NotImplementedError` | A guest frame is running, and the script reached for a capability this sandbox does not have. Under `ScriptError` rather than `StandardError`, so a bare `rescue` does not swallow it |
| Dispatch return value | `Kobako::Transport::Error` | A codec that wrote the Call owes its answer; refusing here leaves the exchange half-served rather than a feature unoffered |
| `#run` arguments, invocation value | `Kobako::SandboxError` | No guest frame is left to raise into, so the absence reaches the host as the invocation failing |

The floor is required but unenforceable: a codec still returns this refusal from the value position if it chooses, so the invocation value and both block-value positions answer it rather than treating it as unreachable.
