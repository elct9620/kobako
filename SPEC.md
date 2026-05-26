# SPEC.md — kobako

## Intent

### Purpose

kobako provides Ruby applications with an in-process, low-cold-start sandbox for executing untrusted mruby code, filling a gap in the Ruby ecosystem where no embeddable, self-hostable isolation runtime exists comparable to V8 isolates in JavaScript.

### Users

| User | Goal |
|------|------|
| Host App developer (Rails / Rack / Sidekiq / CLI) | Execute untrusted or third-party Ruby logic inside their application without risking process integrity or data leakage |
| LLM agent framework author | Run dynamically generated Ruby code produced by a model in a safe, in-process environment and retrieve structured results |
| Teaching platform / CI system operator | Evaluate user-submitted Ruby scripts in isolation without provisioning full containers |
| No-code / low-code tool builder | Evaluate untrusted Ruby expressions (e.g., formula fields, webhook filter rules) safely within their platform |

### Impacts

kobako delivers the following observable behaviors:

- A Host App can execute arbitrary mruby source supplied at runtime (`Sandbox#eval`) and receive a structured result or a categorized error — without any guest code affecting host memory, I/O, or credentials.
- A Host App can register named snippets — either mruby source or pre-compiled RITE bytecode — on a Sandbox at setup time (`Sandbox#preload`), then dispatch into a named entrypoint constant defined by those snippets (`Sandbox#run`) for setup-once / dispatch-many workloads.
- A Host App can inject named Ruby service objects that guest code can call through the Transport layer; those objects are the only mechanism by which guest code can interact with external resources.
- A Host App can define Service methods that accept a guest-supplied block and synchronously yield to it; the block body executes inside the Wasm guest with the same isolation guarantees as the rest of the guest code, and `break` / exception outcomes from the block flow into the same three-class error taxonomy.
- Errors produced during guest execution are attributable to one of three distinct origins (Wasm trap, sandbox/wire fault, or guest application error), enabling the Host App to handle each case differently.
- Guest stdout and stderr are captured and exposed separately from the Transport channel, allowing Host Apps to surface guest logs without confusing them with protocol messages.

### Non-Goals

The following are explicitly outside the scope of kobako:

- LLM integration, agent frameworks, or prompt engineering
- A general-purpose wasmtime Ruby gem
- mruby upstream development or distribution
- Multi-tenant billing, SLA management, or deployment/operations tooling
- Multi-tenant quota / billing logic, cross-Sandbox fairness scheduling, or cross-invocation aggregate resource metrics (the in-Sandbox per-invocation wall-clock timeout and linear memory cap from B-01, together with the per-invocation usage observability that mirrors those caps from B-35, are in scope; cross-Sandbox or cross-invocation aggregation is not)
- Async or yield-resume execution models and interpreter state snapshot/resume

### Core Abstractions

These five roles describe the system. All design and behavior content in later layers uses these names exclusively.

| Role | Responsibility | Scope |
|------|---------------|-------|
| **Host App** | The Ruby application (Rails / Rack / CLI) that uses kobako; holds all credentials and policy | Out of scope — must be named but not designed here |
| **Host Gem** | The kobako gem itself: Ruby API layer (`lib/`) + private native extension (`ext/`); exposes the sandbox interface, routes Transport requests, and manages Handle lifecycle | In scope |
| **Guest Binary** | `kobako.wasm` — compiled from the `wasm/` Rust crate; contains the mruby interpreter and the guest-side Transport proxy; is the isolation boundary | In scope |
| **Service** | A Host Ruby object injected into the sandbox under a two-level name (`Namespace::Member`); the only mechanism by which guest code can access host resources | In scope |
| **Wire Spec** | The MessagePack contract governing all host↔guest Transport messages; not a runtime object but a shared protocol both sides implement | In scope |

**Key internal concepts** (refined in later layers):

- **Sandbox** (`Kobako::Sandbox`): the runtime unit that instantiates the Guest Binary, injects Services, optionally registers preloaded snippets (mruby source or RITE bytecode), executes either a one-shot mruby source (`#eval`) or an entrypoint dispatch into a preloaded constant (`#run`), and returns a structured outcome or raises a typed error.
- **Handle**: an opaque integer token the guest holds to reference a Ruby object the wire codec cannot transmit directly. A Handle enters the guest in one of two symmetric ways: as a Service method's return value (guest→host return path) or as a `#run` argument that the host auto-wraps (host→guest argument path). The guest can use it as a dispatch target, pass it as an argument, or invoke methods on it — which dispatch back to the host as further Transport requests — but cannot dereference it to a Ruby value. Handle lifecycle is fully managed by the Host Gem.
- **Namespace / Member**: `Namespace` is a declared grouping visible to guest as a Ruby module; `Member` is a named binding within a Namespace visible to guest as a module constant.
- **Block / Yield**: A guest-side mruby block passed to a Service method call. The block lives inside the Guest Binary; the Host Gem represents it on the host side as a yield proxy that the Service method can invoke via `yield` or `block.call`. Each yield is a synchronous round-trip into the guest that executes the block body and returns its result. Blocks are scoped to the dispatch call they were passed to and are not reusable beyond it.
- **Three-layer error taxonomy**: `Kobako::TrapError` (Wasm trap), `Kobako::SandboxError` (wire or runtime fault), `Kobako::ServiceError` (guest application error) — each with distinct attribution and handling semantics.

---

## Scope

### System Boundary

#### Responsibility — what kobako does / does not do

**Does:**
- Provide an in-process mruby execution environment isolated by a Wasm boundary
- Bundle a curated mruby standard library in the Guest Binary: the core extension gems (Array / Enum / Hash / Numeric / Object / Proc / Range / String / Symbol / Error / Metaprog) plus pure-compute third-party mrbgems that cover common scripting scenarios. Inclusion is gated by a strict allowlist whose security trade-offs (engine-internal risk vs. guest-side I/O exposure) are documented inline with the build config; the allowlist is the single source of truth for stdlib composition.
- Expose a Ruby API for Host Apps to declare Namespaces and bind host objects as callable Members
- Execute a mruby source synchronously via `Sandbox#eval` and return its last expression as a deserialized Ruby value
- Register snippets on a Sandbox via `Sandbox#preload` (mruby source via `code:` plus `name:`, or RITE bytecode via `binary:` alone), then dispatch into a named entrypoint constant via `Sandbox#run(target, *args, **kwargs)` and return the entrypoint's `#call` value
- Route guest-initiated Transport requests to the correct host Service object and return the serialized result
- Represent Ruby objects outside the wire type mapping as opaque Capability Handles in both directions across the boundary — objects returned by Service methods (guest→host return path) and objects supplied as `#run` arguments (host→guest argument path); allow the guest to reference those handles in subsequent calls
- Capture guest stdout and stderr into separate in-process buffers and expose them to the Host App
- Classify every execution failure into exactly one of three typed error classes and raise it to the Host App
- Ship `kobako.wasm` inside the gem alongside a source-only native extension; provide a single build command that produces both artifacts from a clean clone on Linux or macOS
- Maintain a four-layer test suite and five regression benchmarks as release quality gates

**Does not do:**
- LLM integration, agent frameworks, or prompt engineering — the Host App connects kobako to any LLM
- General-purpose Wasm runtime binding — the native extension is a private implementation detail and exposes no Wasm engine types to the Host App or downstream gems
- mruby upstream development or redistribution — kobako consumes a pinned mruby release tarball unchanged
- Bundle any guest mrbgem that grants access to I/O, networking, sleep, random-seed sources, or syscalls beyond compute and memory — the host capability surface is mediated exclusively through Service injection. This exclusion is enforced by the strict allowlist mechanism above, not by sandboxing alone.
- Async or yield-resume execution — all execution is synchronous and blocking; snapshot/resume is not provided
- Multi-tenant billing, SLA management, deployment, or operational tooling
- Windows platform support — Linux and macOS only

#### Interaction — input assumptions / output guarantees

**Input assumptions:**
- The Host App supplies a valid mruby source string to `#eval` at call time, or a valid `target` plus arguments to `#run`
- Service objects provided by the Host App respond to whatever methods guest code will call; kobako does not validate Service shape
- The host machine has Rust/Cargo available to compile the native extension from source at gem install time
- Each `Kobako::Sandbox` instance is owned by a single Ruby Thread for the duration of any invocation (`#eval` or `#run`); concurrent invocations on the same Sandbox instance from multiple Threads are not supported. Distinct Sandbox instances may be used from distinct Threads (see B-22).

**Output guarantees:**
- Every Sandbox invocation (`#eval` or `#run`) either returns a single deserialized Ruby value or raises exactly one of `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError` — no other outcome is possible. The return value is `#eval`'s last-expression value or `#run`'s entrypoint return value.
- Guest stdout and stderr are always available as separate byte buffers after execution and contain no protocol bytes; truncation, when triggered by a configured cap, is observable via separate predicates on the Sandbox and never appears as inline content within the byte streams
- Capability state is fully reset between successive invocations on the same Sandbox instance, regardless of verb
- The `kobako` gem name, the public Ruby class names `Kobako::Sandbox`, `Kobako::Handle`, `Kobako::Namespace`, `Kobako::TrapError`, `Kobako::SandboxError`, `Kobako::BytecodeError`, `Kobako::ServiceError`, `Kobako::SetupError`, `Kobako::ModuleNotBuiltError`, and the documented public methods on those classes — on `Kobako::Sandbox`: `#define`, `#preload`, `#eval`, `#run`, plus the output readers (`#stdout`, `#stderr`), the truncation predicates (`#stdout_truncated?`, `#stderr_truncated?`), the usage reader (`#usage`, per B-35), and the configuration readers that report how the instance was constructed — `#wasm_path`, `#options` (the `Kobako::SandboxOptions` value object), and the four per-invocation cap readers `#timeout` / `#memory_limit` / `#stdout_limit` / `#stderr_limit` that forward to it; on `Kobako::Namespace`: `#bind` — are stable public contracts. `Kobako::Handle` is named publicly so Host Apps can pattern-match on it inside a `rescue` block, but its constructor is internal to the Host Gem — Handles enter Host App code only as fields on raised error instances, never via direct construction

#### Control — what kobako controls / depends on

**Controls:**
- The entire guest execution environment: mruby interpreter lifecycle, Wasm memory, and capability state
- Handle lifecycle — the guest holds only an opaque integer ID; the Host Gem owns the mapping from ID to host object and all allocation/deallocation decisions
- The host↔guest message codec: MessagePack encoding with three registered ext types (Symbol `0x00`, Capability Handle `0x01`, Fault envelope `0x02`)
- Error attribution: the decision logic that maps execution outcomes to the three error classes

**Depends on:**
- A Wasm execution engine (via the private native extension)
- A pinned mruby release tarball as the guest language runtime embedded in `kobako.wasm`
- A WASI-compatible toolchain available on Linux and macOS to build kobako.wasm
- Host-side and guest-side codec implementations maintained independently; round-trip fuzz tests are the consistency guarantee
- Host App to define and inject Service objects; kobako does not constrain Service shape or method signatures

---

### Feature List

The following features constitute the complete observable surface of the `kobako` gem. Behavior details for each feature are specified in the Behavior layer.

| # | Feature | Role |
|---|---------|------|
| F-01 | Sandbox instantiation | Host Gem |
| F-02 | Namespace declaration | Host Gem |
| F-03 | Member binding | Host Gem |
| F-04 | Synchronous mruby source execution (`#eval`) | Host Gem + Guest Binary |
| F-05 | Guest-initiated Transport dispatch | Host Gem + Wire Spec |
| F-06 | Capability Handle encoding and referencing | Host Gem + Wire Spec |
| F-07 | Three-class error attribution and raising | Host Gem |
| F-08 | Guest output capture | Host Gem + Guest Binary |
| F-09 | Host–guest message codec | Wire Spec (both sides) |
| F-10 | Reproducible build pipeline | Build tooling |
| F-11 | Multi-layer test and benchmark suite | Quality pipeline |
| F-12 | Guest block reception and host-initiated yield re-entry | Host Gem + Guest Binary + Wire Spec |
| F-13 | Snippet preloading — source or bytecode (`#preload`) | Host Gem + Guest Binary |
| F-14 | Synchronous entrypoint dispatch (`#run`) | Host Gem + Guest Binary |

---

### User Journeys

The following journeys describe the primary ways actors use kobako end-to-end. Each journey is a discrete, runnable scenario that covers one or more Impacts stated in Intent.

---

#### J-01 — LLM agent author runs model-generated code with curated capabilities

**Context**
An LLM agent framework author has a pipeline that feeds model-generated Ruby sources to kobako at runtime. The Host App holds credentials (API keys, database connections); the generated sources are untrusted and structurally unknown in advance. The author needs structured results back and must ensure no generated source can exfiltrate credentials or corrupt host state.

**Action**
1. The Host App creates a `Kobako::Sandbox` and declares Namespaces for the capabilities the generated sources may legally call (e.g., a key-value lookup, a write-access log sink).
2. For each model-generated source, the Host App calls `Sandbox#eval` with the source string, passing no additional configuration at call time.
3. The Host App reads the return value of `#eval` as the structured result of the source's final expression.

**Outcome**
The Host App receives a deserialized Ruby value for every successful execution. Generated sources that exceed their declared capabilities receive a `Kobako::ServiceError` (undefined member), sources with Ruby errors raise `Kobako::SandboxError`, and Wasm-level failures raise `Kobako::TrapError` — the agent framework routes each class differently (retry, log, restart sandbox). At no point can a generated source read host memory or call methods not bound as Members.

---

#### J-02 — Host App developer integrates kobako into an existing service

**Context**
A Host App developer is adding kobako to a running Rails or Rack application for the first time. They need to understand the one-time configuration steps and the per-request execution contract before writing any business logic.

**Action**
1. The developer adds kobako to the project's gem dependencies and installs it; the native extension compiles from source.
2. The developer creates a `Kobako::Sandbox`, calls `define` to declare one or more Namespaces, and calls `bind` on each namespace to attach host objects as named Members.
3. At request time, the developer calls `Sandbox#eval` with a source string and uses the return value as the execution result; they also read `Sandbox#stdout` and `Sandbox#stderr` for any guest log output.
4. The developer repeats step 3 for subsequent requests on the same Sandbox instance.

**Outcome**
The developer observes that the same Sandbox instance correctly resets capability state between invocations — a Handle issued during one call is not reachable in the next. The Service objects bound at setup time remain active across all invocations without re-registration. The developer can integrate kobako into request-handling middleware or background job workers using this setup-once / invoke-many pattern.

---

#### J-03 — Teaching platform operator evaluates student submissions in isolation

**Context**
A teaching platform or CI system operator receives student-submitted Ruby sources for automated evaluation. Each submission must run in strict isolation: a failing or malicious submission must not affect the evaluation of any other submission, and no submission may access the host filesystem, network, or credentials.

**Action**
1. For each submission, the operator creates a fresh `Kobako::Sandbox`.
2. The operator optionally binds a grading Service that exposes read-only test fixtures and nothing else.
3. The operator calls `Sandbox#eval` with the student's source string and collects the return value and `Sandbox#stdout` / `Sandbox#stderr` output.
4. The operator repeats this for each submission without restarting the host process.

**Outcome**
Each submission executes inside an isolated Wasm boundary. A submission that crashes or attempts to escape receives a `Kobako::TrapError` (or one of its subclasses) or `Kobako::SandboxError`; neither outcome affects subsequent submissions. Each Sandbox enforces a configurable per-invocation wall-clock timeout (default 60 s) and linear-memory delta cap (default 1 MiB) that bounds how far guest `memory.grow` may push past the linear-memory size observed when the invocation entered; submissions exceeding either raise `Kobako::TimeoutError` or `Kobako::MemoryLimitError` respectively, and never block the calling thread beyond the configured timeout. The Host App owns higher-level policy (queue-level fairness, per-student daily caps, retry semantics) above these per-invocation caps. The operator receives the result value and captured output for every submission that completes. No submission can read another submission's guest output or access host resources beyond the bound grading Service.

---

#### J-04 — No-code platform evaluates user-defined expressions per request

**Context**
A no-code or low-code platform builder allows end users to write Ruby expressions in formula fields or webhook filter rules. These expressions are evaluated on every incoming event or record. The platform needs sub-second evaluation latency, per-user capability scoping, and the guarantee that a broken user expression does not disrupt the platform's own process.

**Action**
1. The platform creates one `Kobako::Sandbox` per tenant, binding a Member that exposes the current record or event payload as a read-only object.
2. On each incoming event, the platform calls `Sandbox#eval` with the user's expression string.
3. The platform reads the return value as the expression result and uses it to drive downstream logic (filter pass/fail, computed field value).

**Outcome**
User expressions that produce a valid Ruby value return it as a deserialized result. Expressions with syntax or runtime errors raise `Kobako::SandboxError`, which the platform surfaces to the user as an expression error without disrupting other tenants. Because each Sandbox's state fully resets between invocations, a user cannot accumulate state across evaluations. Subsequent evaluations on the same Sandbox instance do not incur the cold-start cost of the first execution.

---

#### J-05 — Host App developer distinguishes and handles the three error classes

**Context**
A Host App developer is adding error handling to an existing kobako integration. They need to respond differently to execution failures depending on whether the failure originates in the Wasm engine, the sandboxed guest code itself, or a Service call made by the guest.

**Action**
1. The developer wraps `Sandbox#eval` or `Sandbox#run` in a rescue block that catches `Kobako::TrapError`, `Kobako::SandboxError`, and `Kobako::ServiceError` as separate branches.
2. For `TrapError`, the developer logs the failure and recreates the Sandbox before the next invocation.
3. For `SandboxError`, the developer records the error as a code-level fault (wrong guest code, not broken infrastructure) and surfaces it to the code's author.
4. For `ServiceError`, the developer treats it as a capability-level fault (the guest called a Service correctly but the Service reported an error) and applies the same retry or alerting policy as any other service failure in the Host App.

**Outcome**
The developer can route each failure class through the Host App's existing error-handling infrastructure without inspecting error messages. The three-class taxonomy gives the developer a reliable signal for triage: infrastructure fault (TrapError), authored-code fault (SandboxError), or downstream-service fault (ServiceError). This attribution is guaranteed by kobako regardless of whether the failure originated in an `#eval` source or a `#run` entrypoint.

---

#### J-06 — Host App exposes a block-yielding Service

**Context**
A Host App developer is building a Service that iterates over a collection on the host side and wants each element to be processed by a guest-supplied block (similar to `Array#each` semantics). The Service's natural Ruby form takes a block; the developer wants guest code to call it as `MyEach.run(items) { |x| ... }` without learning a different API for the sandboxed environment.

**Action**
1. The developer defines a Ruby class whose method accepts a block (`def run(items, &blk); items.each { |x| yield x }; end`) and binds an instance under a Namespace Member.
2. The guest writes `Service::MyEach.run([1, 2, 3]) { |x| x * 2 }` — the block is part of the guest code, not part of the host code.
3. The Host App calls `Sandbox#eval` (or `#run` into an entrypoint that contains the call site) and reads the return value.

**Outcome**
The Service method's `yield x` invokes the guest block once per iteration, returning each block result back to the host method as the value of `yield`. The Service method observes its block as an ordinary Ruby Proc with loose arity; the guest-side block executes inside the Wasm sandbox and remains isolated from host state. A `break` inside the guest block terminates the Service method early with the break value, matching standard Ruby semantics. A `next` (or natural fall-through) returns the block value to `yield` and execution continues. Exceptions raised inside the block propagate to the `yield` point where the Service method may rescue or let them flow up. The developer writes the Service in idiomatic Ruby; the sandbox boundary is invisible from the Service method's perspective.

---

#### J-07 — Host App preloads a worker and dispatches many invocations

**Context**
A Host App developer is building a request handler whose business logic is supplied as a Ruby source loaded from disk or a config store. The source defines a stable "worker" entrypoint that processes one request per invocation. The developer wants to pay parsing cost once at setup time, then dispatch many requests through the same Sandbox without re-sending the source on every call.

**Action**
1. The developer creates a `Kobako::Sandbox`, calls `define` and `bind` to expose Services, then calls `sandbox.preload(code: source, name: :Worker)` once at startup. The `:Worker` source defines a top-level constant `Worker` that responds to `#call(request, opts = {})`.
2. At request time, the developer calls `sandbox.run(:Worker, request, **opts)` for each incoming request.
3. The developer reads the return value as the worker's response and reads `Sandbox#stdout` / `Sandbox#stderr` for any guest log output.

**Outcome**
The `Worker` snippet replays into a fresh `mrb_state` before every invocation, so per-invocation isolation holds (B-03) — no state from request N leaks to request N+1. The host normalizes the `:Worker` Symbol, resolves it on top-level `Object`, and dispatches into `Worker.call(request, opts)`; the return value flows back as an ordinary Ruby value. Backtraces produced inside `Worker.call` are attributed to `(snippet:Worker):line`, giving the developer a clear locator. Errors follow the same three-class taxonomy as J-05; `Kobako::SandboxError` with `details: { available: [...] }` surfaces when the developer dispatches a name that the preload table does not provide, allowing immediate diagnosis without inspecting the guest source.

---

## Behavior

The per-anchor behavior table (Initial State → Operation → Result / Final State) for B-01..B-35 and the Error Scenarios subsection covering E-01..E-41 are specified in detail in [`docs/behavior.md`](docs/behavior.md). The decisions below govern those behaviors; consult the linked document for each anchor's full Initial State / Operation / Result / Notes.

- **Four-outcome guarantee:** every Sandbox invocation (`#eval` or `#run`) terminates in exactly one of — a return value, `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError`. No partial completion, no other outcome.
- **Attribution is two-step:** Step 1 — if the Wasm engine reports a trap (including configured-cap traps), raise `Kobako::TrapError` or its named subclass (`Kobako::TimeoutError` per E-19, `Kobako::MemoryLimitError` per E-20). Step 2 — otherwise dispatch on the outcome envelope first-byte tag (`0x01` result, `0x02` panic). Zero-length outcome bytes or unknown tags raise `Kobako::TrapError` as wire-violation fallback.
- **`stdout` / `stderr` never participate in attribution.** They are captured separately and remain readable after error-raising invocations.
- **Setup-time errors split by trigger:** API-misuse cases — invalid `Sandbox.new` cap argument (E-39), invalid Namespace / MemberName patterns (E-16, E-17), `define`-after-first-invocation (E-18), invalid `#run` target (E-24, E-25), invalid `#run` arguments — forged Handle in args / kwargs (E-29) or non-Symbol kwargs key (E-30), invalid `#preload` `name:` / duplicates / post-seal calls (E-33, E-34, E-35) — are Host App programming errors that raise `ArgumentError` or `TypeError` and bypass the attribution pipeline. Content-failure cases — `#preload(code:)` compile error at preload (E-32), preloaded snippet replay failure (E-36), `#preload(binary:)` bytecode structural failure surfaced during first invocation's replay (E-37, E-38) — raise `Kobako::SandboxError` (or its `Kobako::BytecodeError` subclass for bytecode structural failures) with backtrace attribution to the snippet's canonical name when one is available.
- **Construction-time setup failures are a separate class:** `Kobako::Sandbox.new` builds the wasm runtime from `wasm_path` before any invocation runs. An absent or unconstructable artifact (E-40, E-41) raises `Kobako::SetupError` — with the `Kobako::ModuleNotBuiltError` subclass for the unbuilt-artifact case (E-40) — rather than a `TrapError`: no Sandbox is produced, so the four-outcome guarantee and the `TrapError` discard-and-recreate recovery contract apply only after construction succeeds.
- **Snippet replay is uniform across verbs:** preloaded snippets (B-32) replay into the fresh `mrb_state` before every invocation, whether the invocation is `#eval` (then user source loads) or `#run` (then entrypoint resolution happens). B-33 seals the snippet table on the first invocation, parallel to B-07's Service-registration sealing.
- **Anchor groupings:** B-01..B-06 cover Sandbox construction, `#eval` invocation lifecycle, and output capture; B-07..B-11 cover Namespace / Member registration; B-12..B-21 cover guest-initiated Transport dispatch and `Catalog::Handles` lifecycle; B-22 covers per-Thread isolation; B-23..B-30 cover Block / Yield re-entry; B-31 covers `#run` entrypoint dispatch; B-32..B-33 cover `#preload` registration (both `code:` and `binary:` forms) and snippet-table sealing; B-34 covers `#run` host→guest auto-wrap of non-wire-representable arguments into Capability Handles; B-35 covers per-last-invocation usage observability via `#usage`; B-36 covers guest-side `respond_to?` probing on Member / Handle proxies. Errors split across the invocation-outcome classes and the construction-time `SetupError` — `TrapError` (E-01..E-03, E-19, E-20), `SandboxError` (E-04..E-10, E-16..E-18, E-21..E-23, E-26..E-28, E-31, E-32, E-36..E-38 — with E-37 and E-38 raised as the `Kobako::BytecodeError` subclass), `ServiceError` (E-11, E-12, E-13, E-15), `SetupError` (E-40, E-41 — with E-40 raised as the `Kobako::ModuleNotBuiltError` subclass), and setup-time `TypeError` / `ArgumentError` (E-24, E-25, E-29, E-30, E-33, E-34, E-35, E-39).

---

## Refinement

`B-xx` and `E-xx` anchors referenced throughout this layer are defined in detail in [`docs/behavior.md`](docs/behavior.md) per Naming Principle N-8. The current ceiling is B-36 / E-41; subsequent anchors take the next integer above it (B-37, E-42). E-14 is a retired anchor — permanently reserved and never reassigned (N-8).

### Terminology

This section defines every term used in this specification. Each concept has one primary canonical name. Documented aliases are permitted only when both names appear together in this section with the canonical relationship explicitly stated.

---

#### Roles

These five roles describe every actor and artifact in the system. All sections of this specification use these names exclusively.

| Term | Definition | Layer |
|------|-----------|-------|
| **Host App** | The Ruby application (Rails / Rack / Sidekiq / CLI) that uses kobako; holds all credentials, policy, and Service objects. Out of scope for design but referenced throughout. | External |
| **Host Gem** | The `kobako` gem itself: the Ruby API layer (`lib/`) plus the private native extension (`ext/`). Exposes the sandbox interface to the Host App, routes Transport requests, and manages Handle lifecycle. | In scope |
| **Guest Binary** | The file `kobako.wasm`, compiled from the `wasm/` Rust crate. Contains the mruby interpreter and the guest-side Transport proxy. Is the isolation boundary between host and guest execution environments. | In scope |
| **Service** | A Host Ruby object injected into the sandbox under a two-level name (`Namespace::Member`). The only mechanism by which guest code can access host resources. | In scope |
| **Wire Spec** | The MessagePack contract governing all host↔guest Transport messages. Not a runtime object — it is a protocol that both Host Gem and Guest Binary implement independently. | In scope |

*Layer values: **In scope** — designed in this specification; **External** — outside this design, referenced for contract completeness.*

---

#### Internal Concepts

These are sub-components and runtime concepts internal to kobako. They are not exposed as a public API to the Host App unless explicitly stated. The **Catalog** family (`Namespaces`, `Snippets`, `Handles`) holds the Sandbox's setup-time registrations and the per-invocation Handle table. The **Transport** family (`Dispatcher`, `Proxy`, `Request`, `Response`, `Fault`, `YieldProxy`) carries host↔guest messages. **Runtime** is the magnus-wrapped wasm + mruby execution unit; per the Single-Invocation Slot invariant it holds at most one active Invocation per OS thread. **Snapshot** is the per-invocation observable bundle Runtime returns to Sandbox.

| Term | Definition | Public? |
|------|-----------|---------|
| **Sandbox** | The runtime unit instantiated by `Kobako::Sandbox`. Owns one `Runtime`, the three `Catalog` registries (`Namespaces`, `Snippets`, `Handles`), and the output buffers for a single logical execution context. Exposes two invocation verbs — `#eval(code)` for one-shot mruby source execution (B-02 / B-03 / B-06) and `#run(target, *args, **kwargs)` for entrypoint dispatch into a preloaded constant (B-31) — plus the setup verb `#preload` accepting either `code:` plus `name:` for mruby source or `binary:` for RITE bytecode (B-32). Enforces three configurable per-invocation caps — wall-clock timeout, linear memory cap, and per-channel output cap — each independently disableable with `nil`. Exposes per-last-invocation usage observability via `#usage` (B-35) that mirrors the timeout / memory-cap accounting. Maps to the Ruby class `Kobako::Sandbox`. | Yes — `Kobako::Sandbox` is stable public API |
| **Runtime** | The magnus-wrapped wasmtime Store + ABI exports that drive `data/kobako.wasm`. Holds at most one active Invocation per OS thread (Single-Invocation Slot invariant; see `### Implementation Standards` § Invariants for the per-thread statics and per-invocation state it licenses). Returns a Snapshot from each invocation entry point. Receives a dispatch Proc from the Sandbox at construction; the Proc bridges Runtime to the Transport / Catalog layers without Runtime knowing either name. Maps to Ruby class `Kobako::Runtime`. | No |
| **Catalog::Namespaces** | The host-side registry of `Kobako::Namespace` entities. Routes incoming Transport Requests to the resolved Service object. Receives the Sandbox's `Catalog::Handles` by injection so dispatch can resolve Handle targets and arguments without re-owning the table. Maps to Ruby class `Kobako::Catalog::Namespaces`. | No |
| **Catalog::Snippets** | The host-side table of `Kobako::Snippet::Source` / `Kobako::Snippet::Binary` entries, holding source (`code:`) and bytecode (`binary:`) preloads in insertion order. Sealed by the first invocation simultaneously with `Catalog::Namespaces` (B-33). Maps to Ruby class `Kobako::Catalog::Snippets`. | No |
| **Catalog::Handles** | The host-side table allocating `Kobako::Handle` tokens, mapping Handle IDs to Ruby objects. Owned by the Sandbox (B-19) and injected into `Catalog::Namespaces` so dispatch shares the same table the wire layer allocates into. Reset at the start of every invocation (`#eval` or `#run`) and fully discarded when the owning Sandbox is collected. Maps to Ruby class `Kobako::Catalog::Handles`. | No |
| **Transport::Proxy** | The abstract base mruby class for the two guest-side proxy kinds, `Kobako::Member` and `Kobako::Handle`. Maps to mruby class `Kobako::Transport::Proxy`. Guest code does not reference it directly; it sees Member constants under their declared Namespace plus Handle proxies returned from prior dispatches. Proxy defines the shared dispatch path by which a method call on either subclass is forwarded to the host as a Transport Request, and holds no per-instance state of its own. | No |
| **Member** | The guest-side base mruby class for the proxy a bound Member presents to guest code — `Kobako::Member`, a subclass of `Kobako::Transport::Proxy` paralleling `Kobako::Handle`. Each bound Member is installed as a `Kobako::Member` subclass bound to a constant under its Namespace module (e.g., `MyService::KV`); a method call on that constant is forwarded to the host as a Transport Request whose `target` is the constant's `Namespace::Member` path. Has no host-side counterpart — the host represents a Member as a binding entry in `Kobako::Namespace`, not as a class. | No |
| **Snapshot** | The per-invocation observable bundle Runtime returns to Sandbox from `#eval` and `#run`. Carries the raw return bytes, stdout / stderr bytes (with truncation flags), wall time, and memory peak — every observable a single invocation produces. The Sandbox unwraps it into `#stdout`, `#stderr`, `#usage` (B-35), and the decoded return value or raised error per the two-step attribution decision. Maps to Ruby class `Kobako::Snapshot`. | No |
| **Handle** | An opaque integer token used on either side of the wire to reference a Ruby object that is not directly wire-representable — a host-side stateful object returned by a Service call (host→guest return path), or a host-side argument passed into `#run` that requires auto-wrap (host→guest argument path). The guest can pass it as a dispatch target or argument in subsequent calls, and invoking methods on it dispatches as a guest→host Transport request; the guest never sees the underlying Ruby value. Maps to two independent implementations with the same canonical name `Kobako::Handle`: the Ruby class runs in the host process; the mruby class runs inside the Wasm guest as a `Kobako::Transport::Proxy` subclass paralleling `Kobako::Member`, its instances carrying the Handle ID and forwarding method calls to the host as Transport Requests whose `target` is that ID. They share neither code nor instances. | Partially — `Kobako::Handle` instances may surface as fields on raised `SandboxError` or `ServiceError` instances; the Host Gem holds the only constructor (no public `.new`) and Handles enter Host App code only through error fields |
| **Capability Handle** | A Handle that represents a Ruby object outside the wire 12-entry type mapping — either a stateful resource (session, connection, any object the wire codec cannot transmit by value) returned from a Service call, or a non-wire-representable value (`StringIO`, a custom Env / Context object) supplied as a `#run` argument. Transmitted on the wire as MessagePack ext type `0x01`. "Capability Handle" is used when emphasizing the capability-granting semantics; "Handle" is used for brevity elsewhere — both refer to the same concept. | No — same visibility as Handle; no distinct class exists |
| **Block** | A mruby block (or Proc/lambda) the guest passes alongside a Service method call. The block body lives inside the Guest Binary and is never serialized; only its presence is signalled on the wire (Request `block_given` field). Scoped to the single dispatch call that received it — not reusable after that dispatch returns. | No — surfaces only as the `&block` argument the Service method receives |
| **Yield** | A single synchronous round-trip from a Service method into the Block it received. The host Service method invokes `yield` or `block.call` on its block argument; the Host Gem re-enters the Guest Binary, executes the block body, and returns the block's result to the host yield site. Each `yield` is an independent round-trip; a Service method may yield zero or more times during a single dispatch. | No |
| **Yield Proxy** | The host-side Ruby Proc the Host Gem materialises to represent the guest block to the Service method. Has loose Proc-style arity (matches `&block` Ruby conventions). Valid only for the duration of the dispatch that produced it; invocation outside that scope raises (E-23). The Service method may invoke it directly via `block.call` or implicitly via `yield`. Maps to Ruby class `Kobako::Transport::YieldProxy`. | No |

---

#### Error Classes

Three error classes cover every failure outcome of a Sandbox invocation (`#eval` or `#run`). These class names are stable public API and must not be renamed or aliased.

| Term | Ruby Class | Layer it represents | When raised |
|------|-----------|--------------------|----|
| **TrapError** | `Kobako::TrapError` | Wasm engine layer | The Wasm execution engine crashed (OOM, `unreachable`, stack overflow, import signature mismatch) or the wire layer detected a structural violation indicating a corrupted guest runtime (zero-length outcome, unknown outcome tag) |
| **SandboxError** | `Kobako::SandboxError` | Sandbox / wire layer | The guest ran to completion but execution failed due to a protocol fault, a mruby runtime error, or a host-side wire decode failure. The Wasm instance is retired normally; the sandbox infrastructure is intact |
| **ServiceError** | `Kobako::ServiceError` | Service / capability layer | The guest ran to completion, the mruby script itself did not crash, but a Service capability call reported an application-level failure that was not rescued within the script |

The three classes above cover every failure outcome of a Sandbox *invocation*. Construction (`Sandbox.new`) is a setup verb and carries its own error class, raised before any invocation runs and outside the two-step attribution decision:

| Term | Ruby Class | Layer it represents | When raised |
|------|-----------|--------------------|----|
| **SetupError** | `Kobako::SetupError` | Construction / runtime-setup layer | `Sandbox.new` could not construct the wasm runtime from the configured `wasm_path` — an absent or unreadable artifact, bytes that are not a valid Wasm module, or engine / linker / instantiation setup failure. No Sandbox instance is produced, so the four-outcome guarantee and the `TrapError` discard-and-recreate recovery contract do not apply |

**Named subclasses (stable public API):**

| Term | Ruby Class | Superclass | Meaning |
|------|-----------|-----------|---------|
| **TimeoutError** | `Kobako::TimeoutError` | `Kobako::TrapError` | Absolute wall-clock time since the invocation entry (`#eval` or `#run`) reached the configured per-invocation `timeout` (default 60 s); trap fires at the next guest wasm safepoint after the deadline. See E-19; B-01 Notes covers host-callback accounting. |
| **MemoryLimitError** | `Kobako::MemoryLimitError` | `Kobako::TrapError` | Cumulative guest `memory.grow` since invocation entry would push past the configured `memory_limit` (default 1 MiB) — the mruby image's initial allocation and prior invocations' watermark are folded into the per-invocation baseline rather than the budget; see E-20 |
| **HandlerExhaustedError** | `Kobako::HandlerExhaustedError` | `Kobako::SandboxError` | Handle ID counter in `Catalog::Handles` reached `0x7fff_ffff` (2³¹ − 1) within a single invocation; further allocation is impossible |
| **BytecodeError** | `Kobako::BytecodeError` | `Kobako::SandboxError` | `#preload(binary:)` bytecode failed structural validation during the first invocation's snippet replay against a fresh `mrb_state` — RITE version mismatch (E-37) or corrupt body (E-38). Backtrace attributes to the bytecode's `debug_info` filename when the bytecode carries one. |
| **ModuleNotBuiltError** | `Kobako::ModuleNotBuiltError` | `Kobako::SetupError` | The Guest Binary artifact is absent at the resolved `wasm_path` — the common state on a fresh clone before `rake compile`. See E-40 |

**Wire-level error string (not a Ruby class):** The string `"Kobako::Transport::WireError"` appears only as the `class` field value in a Panic envelope (defined in [`docs/wire-contract.md`](docs/wire-contract.md) § Outcome Envelope; the governing summary lives below in `### Wire Contract`) to signal that the Transport layer detected a wire violation. On the host side this maps to a raised `Kobako::SandboxError`; there is no standalone `Kobako::Transport::WireError` Ruby class on the host. (The guest mruby class `Kobako::Transport::WireError` exists only to be raised inside the Guest Binary; it is captured by the guest's top-level handler and converted into the panic envelope string.)

---

#### Namespace and Member

These terms describe the two-level injection model used to expose host capabilities to guest code.

| Term | Definition | Guest-visible form |
|------|-----------|-------------------|
| **Namespace** | A named grouping declared by the Host App via `sandbox.define(:Name)`. Namespaces are declared at setup time before the first invocation (`#eval` or `#run`). The namespace itself holds no state — it is a container for Members. Maps to Ruby class `Kobako::Namespace`. | Ruby module (e.g., `MyService`) |
| **Member** | A name→Service binding slot within a Namespace, declared via `namespace.bind(:Name, service)`. The Member is the binding, not the object: it pairs a constant-form name with the Service bound into it. The bound Service (see Roles) is what receives Transport requests dispatched from guest code. | Module constant bound to a `Kobako::Member` subclass (e.g., `MyService::KV`) |

---

### Wire Contract

The abstract logical shape of every host↔guest message exchanged during a `#run` invocation is specified in detail in [`docs/wire-contract.md`](docs/wire-contract.md). It is a Consistency-layer contract: both sides implement it independently, and a kobako gem release ships exactly one version of it. Byte-level encoding (msgpack type mapping, ext code numbers, binary layout) is specified in [`docs/wire-codec.md`](docs/wire-codec.md). The decisions below govern the contract; the linked documents reproduce them as field tables, envelope shapes, and per-position rules.

- **Transport role:** the Guest Binary is the sole initiator of host↔guest communication; the Host Gem responds synchronously within the same Wasm import call frame. There are no callbacks, promises, or yield-resume mechanisms.
- **Request shape:** every Request carries exactly five logical fields — `target` (Member path or Capability Handle reference, distinguishable at the first wire byte), `method` (string), `args` (ordered list, may include Handle references), `kwargs` (Symbol-keyed map; empty kwargs is always present, never absent), and `block_given` (bool indicating the guest call site supplied a block; the block body is never serialized — only the flag travels).
- **Response shape:** mutually exclusive Success (`status=0`, `value`) or Fault (`status=1`, fault envelope) variant. No partial success, no streaming.
- **Capability Handle contract:** opaque on both sides of the boundary (cannot be dereferenced, and the constructor is internal to the Host Gem — neither guest mruby code nor Host App code can fabricate a Handle from a bare integer); host-allocated when a Service returns a non-wire-representable object (B-14, guest→host return path) or when `#run` receives non-wire-representable arguments (B-34, host→guest argument path); scoped to a single invocation (`Catalog::Handles` reset at every `#eval` or `#run`); ID bounded by `0x7fff_ffff` (allocation beyond raises `Kobako::SandboxError`, no silent wraparound).
- **Fault envelope:** `type` (one of three reserved values — `"runtime"`, `"argument"`, `"undefined"` — stable across releases), `message` (string), `details` (optional structured data). Adding a new `type` requires a coupled host+guest release.
- **Outcome envelope:** per-invocation final result, distinct from per-dispatch Response; two variants — Result envelope (serialized last mruby expression) or Panic envelope (`origin` / `class` / `message` / `backtrace` / `details`, where `origin="service"` attributes to `Kobako::ServiceError` and `origin="sandbox"` or absent attributes to `Kobako::SandboxError`; `details` is optional structured data carrying caller-actionable diagnostics such as `available` constant lists for E-27). Zero-length outcome bytes or an unrecognized tag raises `Kobako::TrapError`.
- **Yield round-trip:** the host-initiated counterpart of a Request/Response. The Host Gem re-enters the guest synchronously when a Service method invokes its yield proxy (B-24); yield round-trips nest strictly within the dispatch frame that produced the proxy and stack in LIFO order; each frame holds at most one proxy.
- **YieldResponse envelope:** three live tag values — `0x01` ok (block returned a wire-legal value), `0x02` break (block executed `break val` from a non-lambda context, unwinds the Service method per B-25), `0x04` error (block raised, returned an unrepresentable value, used `return` from a non-lambda block, or invoked an escaped yield proxy — E-21..E-23). `0x03` is reserved and rejected as a wire violation.
- **Release-internal contract:** the Wire Spec ships in a single kobako gem release that updates both sides simultaneously. No in-band version field, no negotiation mechanism, no one-sided evolution; the single wire shape defined in the release is the only shape either side implements. Wire-affecting changes are recorded in `CHANGELOG.md` under Breaking Changes.

---

### Wire Codec

The byte-level codec that pins the Wire Contract is specified in detail in [`docs/wire-codec.md`](docs/wire-codec.md). The decisions below govern the codec; the linked document reproduces them as binary layouts, hex examples, and per-position rules.

- **Codec:** MessagePack on both sides of the Wasm boundary; no fallback or alternative codec is permitted. All Requests, Responses, and Outcome envelopes are MessagePack-encoded byte sequences.
- **Wire type set:** exactly 12 entries — `nil`, `bool`, `int`, `float`, `str`, `bin`, `array`, `map`, and the three ext codes `0x00` (Symbol), `0x01` (Capability Handle), `0x02` (Fault Envelope). Any msgpack type or ext code outside this set is a wire violation; both sides reject without attempting to decode.
- **Ext code assignments are stable for the life of a release:** `0x00` Symbol (kwargs map keys MUST be this type), `0x01` Capability Handle (`fixext 4`, big-endian u32 ID, cap `0x7fff_ffff`), `0x02` Fault Envelope (embedded msgpack map carrying `type` / `message` / `details`).
- **Envelope framing:** Request is a 5-element msgpack array (`target`, `method`, `args`, `kwargs`, `block_given`); Response is a 2-element msgpack array (`status`, `value`-or-fault); Result envelope emits the value directly (Outcome tag discriminates); Panic envelope is a msgpack map (keys `origin`, `class`, `message`, `backtrace`, `details`); Outcome envelope is a one-byte tag (`0x01` result / `0x02` panic) followed by its payload; YieldResponse envelope is a one-byte tag (`0x01` ok / `0x02` break / `0x04` error; `0x03` reserved) followed by an optional payload.
- **ABI surface:** the wire ABI is a closed enumerated set — one host import (`__kobako_dispatch`) and exactly five guest exports (`__kobako_eval`, `__kobako_run`, `__kobako_alloc`, `__kobako_take_outcome`, `__kobako_yield_to_block`). Function names and Wasm signatures are fixed across a release; growth requires a new SPEC anchor that lifts the enumeration. `__kobako_dispatch`, `__kobako_take_outcome`, and `__kobako_yield_to_block` return a packed i64 carrying `ptr` (high 32 bits) and `len` (low 32 bits). `__kobako_eval` and `__kobako_run` are the two invocation entry points; both write a single Outcome envelope to OUTCOME_BUFFER before returning, and the host applies the two-step attribution decision to whichever export it called.
- **Single-dispatch payload cap:** 16 MiB in either direction. Exceeding the cap is a wire violation; the Host Gem walks the trap path.
- **Consistency guarantee:** round-trip fuzz between the two independently-implemented codecs (Ruby host, Rust/mruby guest) is the sole correctness mechanism, covering all 12 wire types and all 3 ext types in both directions. Any round-trip failure is a wire regression that blocks release. The harness contract is specified in Implementation Standards § Testing Style.

---

### Naming Principles

The following principles govern how all names in this specification and in the `kobako` public surface are formed. They are declarative rules, not rationale.

| # | Principle | Applies to |
|---|----------|-----------|
| N-1 | Role names are PascalCase with every word capitalized: `Host App`, `Host Gem`, `Guest Binary`, `Wire Spec` | All role names in this document and in code comments |
| N-2 | All public Ruby classes and modules live under the `Kobako::` namespace | Ruby classes: `Kobako::Sandbox`, `Kobako::TrapError`, `Kobako::SandboxError`, `Kobako::ServiceError`, `Kobako::Handle`, `Kobako::Namespace` |
| N-3 | The gem name is always lowercase: `kobako` | Gemspec, `require` statements, Bundler references |
| N-4 | The Wasm artifact name is fixed: `kobako.wasm` | Build output, gem packaging, documentation |
| N-5 | Internal Rust crates are named with a hyphen prefix matching the gem: `kobako-wasm` (Guest Binary crate), `kobako-ext` (native extension crate) | `Cargo.toml` package names; not exposed to Ruby |
| N-6 | A concept has exactly one name; no synonyms appear in the same document or public surface | All layers of this specification |
| N-7 | Error class names encode the layer they represent: `TrapError` → Wasm engine layer, `SandboxError` → sandbox/wire layer, `ServiceError` → service/capability layer | `Kobako::TrapError`, `Kobako::SandboxError`, `Kobako::ServiceError` |
| N-8 | `B-xx` and `E-xx` anchors are assigned monotonically and append-only across the spec corpus (`SPEC.md` plus `docs/*.md`); existing anchors are never renumbered, and a new entry always takes the next integer above the current ceiling regardless of which document or subsection it belongs to. A number freed by a retired anchor is a permanent tombstone — it is never reassigned, so a historical reference to a retired number never rebinds to unrelated content. Relocation of a contiguous anchor range from `SPEC.md` to `docs/<topic>.md` during a TOC-mode extraction is not a renumbering and is permitted | All Behavior and Error Scenario entries |

---

### Implementation Standards

#### Architecture

The kobako codebase is split into two top-level source areas with a strict boundary between them:

- **`lib/`** — the Host Gem Ruby surface. Contains `kobako.rb` (the main entry point that loads the native extension and defines the public API) and `lib/kobako/` sub-modules (error class definitions, codec helpers, Transport value objects and Dispatcher, Catalog registries — `Namespaces` / `Snippets` / `Handles`). This is the only layer the Host App interacts with directly.
- **`ext/kobako/`** — the private native extension (`kobako-ext` Rust crate). Wraps wasmtime, owns the Wasm engine lifecycle, and implements the host-side import function `__kobako_dispatch`. This is a private implementation detail of the Host Gem; it is never intended as a reusable wasmtime binding and exposes no Wasm engine types to the Host App or downstream gems.
- **`wasm/`** — the Guest Binary source (`kobako-wasm` Rust crate, target `wasm32-wasip1`). This is build-time only; it is compiled to `data/kobako.wasm` and excluded from the published gem alongside build tools (`vendor/`, `tasks/`, `build_config/`).
- **`data/kobako.wasm`** — the pre-built Guest Binary artifact. Produced at release time on the publisher's machine and shipped inside the gem. End users receive this file at install time; they never need to recompile the Wasm side.

The boundary rule is: **`ext/` is private to the Host Gem and must never be imported by downstream gems**; `lib/` is the stable public surface. The host-side build (`ext/`) and the guest-side build (`wasm/`) maintain independent Cargo workspaces and separate lock graphs. The root `Cargo.toml` contains only `ext/kobako` in `members` and excludes `wasm/` and `vendor/` — this isolation prevents host-only crates (e.g., `wasmtime`) from appearing in the wasm32 dependency graph.

#### Design Patterns

The following patterns are enforced project-wide and apply at every layer:

- **Wire is a release-internal contract** — see `### Wire Contract` § release-internal contract for the governing statement. Design implication: never add a wire field gated on a flag both sides do not compile in the same release; treat the Wire Spec as a single coupled artifact.
- **Round-trip fuzz is the consistency guarantee.** The host-side codec is implemented in pure Ruby under `lib/kobako/codec/` and is loadable at `require` time before the native extension is available; the guest-side codec is implemented in Rust under `wasm/kobako-wasm/src/codec/` for the `wasm32-wasip1` target. The two implementations share no source code — the deployment model (the gem must `require` cleanly without a built native extension, and `wasm32-wasip1` cannot embed Ruby) forbids a single codec. Correctness is established by bidirectional round-trip fuzz covering all 12 wire types and all three ext types.
- **Codec depends on value objects.** The Codec layer registers `Kobako::Handle` as its ext 0x01 decode target and `Kobako::Fault` as its ext 0x02 decode target. Both are top-level value objects (not nested under `Transport`) precisely so this holds: the dependency direction is Codec → value objects; neither the value objects nor the Transport layer depends on Codec. This makes the value objects loadable without the codec available and keeps the codec a pure transformation over a known set of host-side types.
- **Three-layer error attribution is two-step** — see `## Behavior` § attribution for the governing Step 1 / Step 2 decision. Design implication: error classification is a pure function of `(trap?, outcome_tag)`; exit codes, stdout, and stderr are never inputs to that function at either step.
- **Source-only distribution.** The published gem does not include precompiled native extensions for any platform. End users compile `ext/kobako/` from Rust source using their local Rust toolchain and cargo. The only pre-built binary artifact shipped in the gem is `data/kobako.wasm`.
- **Build-time vendor isolation.** `vendor/wasi-sdk/` and `vendor/mruby/` are fetched from official release tarballs at build time and are never committed to the repository. Version numbers are pinned as constants inside `tasks/vendor.rake`. This avoids git submodule pointer maintenance and guarantees cross-environment reproducibility.
- **Fix the bottom layer, not the top.** When a gap is found in a low-level interface (codec type coverage, setjmp/longjmp flag, Wire Spec field, `Catalog::Handles` guard, Panic envelope schema), the fix is applied to the interface layer itself. Working around a low-level gap in a higher-level capability or application layer is not permitted.
- **Process-scope Engine and Module cache.** The wasmtime Engine and the compiled Module for `data/kobako.wasm` are cached at process scope by the native extension. The first `Kobako::Sandbox` constructed in a process pays Engine init and Module compile; every subsequent Sandbox in the same process — regardless of which Thread constructs it — amortizes against this shared state. The cache is implicit; the Host App has no API to inspect, warm, or invalidate it. This pattern is what makes the Sandbox-per-tenant and Sandbox-per-Thread shapes (B-22) practical.

##### Invariants

The following invariants hold across every layer of the system. Each is a hard rule; no layer may violate them.

| Invariant | Applies to | Enforcement |
|-----------|-----------|-------------|
| The terms `Namespace` and `Member` (not "tool" or generic names) are used everywhere in code, documentation, and wire values | All layers | Documentation |
| Wire `target` for dispatch requests uses the Ruby constant-path form `"Namespace::Member"`; Handle references use ext 0x01 — both forms are distinguishable at the first wire byte | Wire Spec, both codec implementations | Test-time |
| Error attribution is determined solely by `(trap?, outcome_tag)` — stdout, stderr, and exit codes are excluded from attribution logic | Host Gem, error handling | Test-time |
| stdout and stderr carry only user-observable guest output; no kobako protocol bytes appear on these channels | Guest Binary, Host Gem | Test-time |
| `#stdout` and `#stderr` byte content never includes truncation sentinels; truncation status is observable only via `#stdout_truncated?` / `#stderr_truncated?` | Host Gem | Test-time |
| An invocation (`#eval` or `#run`) exceeding the configured `timeout` raises `Kobako::TimeoutError` via the trap-attribution path; no other outcome is possible for wall-clock cap exhaustion | Host Gem | Runtime |
| Guest `memory.grow` whose per-invocation delta past the entry-time linear-memory baseline exceeds the configured `memory_limit` traps unconditionally and raises `Kobako::MemoryLimitError`; the host never observes a silent `memory.grow` failure from cap exhaustion | Host Gem | Runtime |
| `Sandbox#usage` reports the most recent invocation's `wall_time` (Float seconds the guest export call spent inside wasmtime) and `memory_peak` (Integer bytes, high-water of the per-invocation `memory.grow` delta past the entry-time baseline). Both readers share their accounting boundary with the matching cap from B-01: `wall_time` includes Service-callback time, and `memory_peak` excludes the mruby image's initial allocation and any prior-invocation watermark. Both readers are populated regardless of outcome (value return, `TrapError`, `SandboxError`, `ServiceError`); `memory_peak` never exceeds `memory_limit` on `MemoryLimitError`. Pre-invocation reads return `Kobako::Usage::EMPTY` | Host Gem | Runtime |
| `Sandbox#eval` returns the last mruby expression value and `Sandbox#run` returns the entrypoint's `#call` value, both via the Result envelope path; objects without a wire representation take the Panic envelope path — no implicit `inspect` or `to_h` conversion | Guest Binary, Wire Spec | Test-time |
| `vendor/` is never committed to the repository; build tools fetch release tarballs at build time | Repository, task scripts | Build-time |
| mruby exception unwind is implemented via wasi-sdk setjmp/longjmp (three mandatory compiler flags); direct modification of mruby setjmp call sites is not permitted | Guest Binary build | Build-time |
| Guest Binary target is `wasm32-wasip1`; wasi-preview2 and component model are out of scope | Guest Binary build, Host Gem | Build-time |
| `Catalog::Handles` IDs are bounded by `0x7fff_ffff` (2³¹ − 1); exceeding the cap raises `Kobako::SandboxError` immediately — no silent wraparound or truncation | Host Gem, wire layer | Runtime |
| `ext/kobako/` is a private binding for the kobako gem only; no downstream gem may depend on it directly | Architecture | Documentation |
| Handle lifecycle is per-invocation: `Catalog::Handles` is fully cleared and the counter reset to 1 at the start of every invocation (`#eval` or `#run`); Handles from invocation N are invalid in invocation N+1 | Host Gem, Wire Spec | Test-time |
| Handles are never individually released by the guest; the host implementation does not use `ObjectSpace.define_finalizer` for `Catalog::Handles` entries | Host Gem | Documentation |
| Single-Invocation Slot: Runtime holds at most one active Invocation per OS thread for the duration of any invocation (`#eval` or `#run`). Nested host→guest dispatch (Service calls Service via a yielded block which calls another Service) shares the same Invocation; nested dispatch frames stack within it (B-28). There is no stack of Invocations — at most one per thread. The slot licenses the guest-side per-thread statics (`MRB` slot, `BLOCK_STACK`, `OUTCOME_BUFFER`) and the host-side per-invocation state (active caller pointer, deadline, wall-clock entry, memory peak, captures) | Runtime, Host Gem | Runtime |
| `Kobako::Sandbox#preload` accepts two forms — `code:` plus `name:` (matching `/\A[A-Z]\w*\z/`) for source, or `binary:` alone for RITE bytecode. The host treats `binary:` payloads as opaque bytes; the snippet's canonical name, when present, lives in the bytecode's `debug_info` and is read by the guest at load time. Bytecode compiled without a `debug_info` section (e.g., `mrbc` without `-g`) is accepted; frames originating in such a snippet are omitted from `Exception#backtrace` per upstream mruby semantics, with exception class, message, and `origin` attribution preserved. Structural failures of `binary:` payloads (RITE version mismatch, corrupt body) surface as `Kobako::BytecodeError` during the first invocation's snippet replay. The snippet table is sealed by the first invocation (`#eval` or `#run`), simultaneously with the Service-registration seal (B-07 / B-33). | Host Gem | Runtime |
| `Kobako::Sandbox#run(target, ...)` resolves `target` (Symbol or String, normalized to Symbol) only as a top-level `Object` constant; `::`-segmented names and any other multi-segment form fail the constant pattern at host pre-flight | Host Gem | Runtime |
| Wire ABI is a closed enumerated set: exactly one host import (`__kobako_dispatch`) and exactly five guest exports (`__kobako_eval`, `__kobako_run`, `__kobako_alloc`, `__kobako_take_outcome`, `__kobako_yield_to_block`); each entry's name and Wasm signature is fixed across a release. Adding a new import or export requires a new SPEC anchor that lifts the enumeration — the closed-set rule itself is unchanged. | Wire Spec, both codec implementations | Build-time |
| Yield round-trip nests strictly within the dispatch frame whose Service method initiated it; nested dispatch frames each receive at most one yield proxy and the proxies stack in LIFO order — they are not interchangeable across frames | Wire Spec, Host Gem | Runtime |
| Guest mruby's `MRB_STR_LENGTH_MAX` is 1 MiB — a guest-side String at or above this size raises `ArgumentError` inside the guest. This is independent of the 16 MiB single-dispatch wire payload limit; a wire payload can approach the 16 MiB cap via composite values (Array, binary), but a single guest String value cannot. | Guest Binary build (mruby config) | Runtime |

#### Testing Style

The test suite is organized into four layers. All four layers must exist and must pass before a release is approved. No single layer may substitute for another.

| Layer | Name | Scope | When it must pass |
|-------|------|-------|------------------|
| 1 | **Codec round-trip fuzz** | Bidirectional wire codec agreement between Host Gem and Guest Binary codec implementations; covers all 12 wire types, all three ext types, and nested compositions | Always — any failure is a wire regression that blocks release unconditionally |
| 2 | **Wire integration** | Full Request / Response exchange through a live Sandbox, including all envelope type variants | Before release |
| 3 | **Ext unit** | `ext/kobako/` internal Rust unit tests and `lib/kobako/` Ruby specs without starting a Sandbox; includes `Catalog::Handles` allocation / release / fetch, `HandlerExhaustedError` guard at `0x7fff_ffff`, wire encode/decode boundary values, and wasmtime API wrapper correctness | Before release; the `Catalog::Handles` exhaustion guard is also a required build-pipeline guard (see below) |
| 4 | **End-to-end** | Full Host App → Sandbox invocation (`#eval` for one-shot source; `#run` for entrypoint dispatch into a `#preload`-registered constant) → Service call → result return path; must cover all three error attribution paths (`TrapError`, `SandboxError`, `ServiceError`) with each trigger, kwargs dispatch (including empty kwargs, symbol-key wire form, and Symbol round-trip through args / return values), Handle chaining (Service returns stateful object, guest uses Handle as subsequent dispatch target), Handle lifecycle over Sandbox teardown, cross-invocation Handle invalidity (a Handle obtained in invocation N used as a target in invocation N+1 surfaces as `Kobako::ServiceError` with `type="undefined"` when not rescued within the guest — see B-18, E-13), block / yield round-trip (Service method receives a block via `&block` and yields one or more times; covers each YieldResponse tag — `0x01` ok, `0x02` break with B-25 unwind semantics, `0x04` error from block exception, and the unsupported-`return` path of E-21 raising at the yield site; covers lambda-block `break` silent return per B-27 and nested dispatch frames per B-28), stdout / stderr isolation from the Transport channel, and the wire-violation edge cases (`len=0`, unknown tag, Result envelope with unrepresentable value) | Before release |

The recommended execution order is Layer 3 → Layer 1 → Layer 2 → Layer 4 (cheapest first; fail fast before starting the Sandbox).

**Layer 1 harness contract** — the Codec round-trip fuzz harness must satisfy two cross-implementer requirements regardless of transport mechanism (in-process FFI, subprocess IPC, or wasmtime-embedded invocation):

- The random seed for each run is sourced from an environment variable, and any failing iteration's failure output includes the seed in use; a failing run is reproducible from the seed alone.
- The generator records which wire types and ext types it exercises; at the end of each run, the harness asserts that all 12 wire types and all three ext types were observed at least once. A coverage gap fails the harness independently of any byte-equality failure.

Iteration count and the transport between the two codec implementations are implementer-chosen.

**Build-pipeline guards** — the following checks must run as part of the build step, before the full test suite:

- `Catalog::Handles` ID cap guard: after `ext/kobako/` is compiled, immediately verify that ID `0x7fff_ffff` is successfully allocated and that the next attempt raises `Kobako::HandlerExhaustedError`.
- Gemspec files whitelist check: after `gem build kobako.gemspec`, verify that the resulting archive does not contain `vendor/`, `wasm/`, `tasks/`, or `build_config/` content.

**Regression benchmarks** — the following five benchmarks must be maintained in `benchmark/` with baseline results stored in git. Each release compares against the previous baseline; a regression greater than +10% requires explicit review and approval before release proceeds.

| # | Benchmark | What it detects |
|---|-----------|----------------|
| 1 | Cold start latency (`Kobako::Sandbox.new` → first invocation, `#eval` or `#run`) | wasmtime Module load / Engine initialization regression |
| 2 | Transport round-trip latency (single minimal Service call) | Wire codec, import function dispatch, `Catalog::Handles` lookup combined |
| 3 | Codec throughput at varying payload sizes and nesting depths (host and guest sides measured separately) | Unnecessary allocations or codec path regressions |
| 4 | mruby script evaluation time (fixed script, no Transport calls) | Impact of `build_config/wasi.rb` flag changes on VM execution speed |
| 5 | Handle allocation and release throughput (bulk Service return value wrapping) | `Catalog::Handles` internal dictionary and counter performance |

Benchmark #1 and #4 are the primary indicators of `build_config/wasi.rb` changes. Benchmark #3 must be run across two dimensions independently: (a) fixed payload size, varying nesting depth; (b) fixed depth, varying payload size. Baseline records are stored as `benchmark/results/<date>-<short-sha>.json`; release baselines are stored under git tags following the pattern `benchmark/<semver>` (e.g., `benchmark/1.0.0`).

#### Code Organization

The following directory layout principles govern the repository. The specific test framework, benchmark library, and CI provider are implementation choices and are not pinned here.

**Directory roles (required, not relocatable):**

- `lib/` — Host Gem Ruby surface; public API entry point and sub-modules
- `ext/kobako/` — private native extension; Rust source (`src/`), `Cargo.toml`, `extconf.rb`, `build.rs`; compiled to `lib/kobako/kobako.<ext>` by rake-compiler
- `wasm/` — Guest Binary Rust source; compiled to `data/kobako.wasm`; excluded from the published gem
- `data/` — pre-built Wasm artifact (`kobako.wasm`); included in the published gem; never manually edited
- `build_config/` — mruby build configuration (`wasi.rb`); build-time only; excluded from the published gem
- `vendor/` — build-time toolchain storage for wasi-sdk and mruby tarballs; not committed; entirely covered by `.gitignore`; excluded from the published gem
- `tasks/` — Rakefile sub-task files (`vendor.rake`, `wasm.rake`, `ext.rake`), each owning one task group; excluded from the published gem
- `spec/` (or `test/`) — test files; excluded from the published gem; one consistent convention across the entire repo
- `benchmark/` — benchmark scripts and baseline result files; excluded from the published gem
- `docs/` — design documentation; excluded from the published gem

**gemspec files whitelist:** `kobako.gemspec` uses an explicit allowlist (not a blacklist) to specify `spec.files`. The published gem includes: `lib/**/*.rb`, `ext/kobako/**/*.{rs,toml,rb,h}`, `data/kobako.wasm`, `Rakefile`, `kobako.gemspec`, `README.md`, `LICENSE`, `CHANGELOG.md`. All other directories (`vendor/`, `wasm/`, `tasks/`, `build_config/`, `docs/`, `benchmark/`, `spec/`, `test/`) are excluded.

**Two build paths, two starting points:**

- *End-user path*: `gem install kobako` → rake-compiler runs `compile_ext` (Rust toolchain required) → `data/kobako.wasm` is already present; wasi-sdk and mruby tarballs are not needed.
- *Developer path*: `git clone` → `bundle install` → `bundle exec rake compile` → Rakefile runs vendor setup (downloads wasi-sdk and mruby tarballs to `vendor/`), then the wasm build (produces `data/kobako.wasm`), then `compile_ext`.

Each task in `tasks/*.rake` must be idempotent: the presence of target files (e.g., `vendor/wasi-sdk/bin/clang`, `vendor/mruby/build/wasm32-wasip1/lib/libmruby.a`) short-circuits re-execution, so incremental development only reruns the changed stage.

**Release documentation — six required artifacts:** A release is not complete until all six of the following documents are present and synchronized with the code. Shipping code before documentation is not permitted.

| # | Document | Contents |
|---|----------|----------|
| 1 | `README.md` | Quickstart (5-line runnable example), API overview, install flow including MSRV |
| 2 | Development guide (`docs/`) | Complete design specification (this document) |
| 3 | Wire Spec | Normative host↔guest codec contract; the binding reference for the Host Gem and Guest Binary implementations shipped in this release |
| 4 | Build guide | Rake task reference, vendor version table, common build error troubleshooting |
| 5 | `CHANGELOG.md` | Keep a Changelog format, generated and maintained by release-please from Conventional Commit messages — never hand-authored. release-please opens a release PR that writes the file and derives its Added / Changed / Fixed / Breaking Changes sections from the `feat` / `fix` / `feat!` / `BREAKING CHANGE:` commit types since the last release; the file first appears with that release PR. |
| 6 | `LICENSE` | License file |

Wire-affecting changes that break round-trip compatibility are recorded by marking their commit as a breaking change (`feat!` / `fix!` or a `BREAKING CHANGE:` footer); release-please rolls these into the CHANGELOG's Breaking Changes section automatically. MSRV changes are treated as breaking changes and marked the same way. The contributor's obligation is the commit-message convention, not editing `CHANGELOG.md` directly.
