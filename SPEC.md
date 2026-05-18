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

- A Host App can execute arbitrary mruby code supplied at runtime and receive a structured result or a categorized error — without any guest code affecting host memory, I/O, or credentials.
- A Host App can inject named Ruby service objects that guest code can call via RPC; those objects are the only mechanism by which guest code can interact with external resources.
- A Host App can define Service methods that accept a guest-supplied block and synchronously yield to it; the block body executes inside the Wasm guest with the same isolation guarantees as the rest of the script, and `break` / exception outcomes from the block flow into the same three-class error taxonomy.
- Errors produced during guest execution are attributable to one of three distinct origins (Wasm trap, sandbox/wire fault, or guest application error), enabling the Host App to handle each case differently.
- Guest stdout and stderr are captured and exposed separately from the RPC protocol channel, allowing Host Apps to surface guest logs without confusing them with protocol messages.

### Non-Goals

The following are explicitly outside the scope of kobako:

- LLM integration, agent frameworks, or prompt engineering
- A general-purpose wasmtime Ruby gem
- mruby upstream development or distribution
- Multi-tenant billing, SLA management, or deployment/operations tooling
- Multi-tenant quota / billing instrumentation, cross-Sandbox fairness scheduling, or per-process aggregate resource metrics (the in-Sandbox per-run wall-clock timeout and linear memory cap from B-01 are in scope; cross-Sandbox aggregation is not)
- Async or yield-resume execution models and interpreter state snapshot/resume

### Core Abstractions

These five roles describe the system. All design and behavior content in later layers uses these names exclusively.

| Role | Responsibility | Scope |
|------|---------------|-------|
| **Host App** | The Ruby application (Rails / Rack / CLI) that uses kobako; holds all credentials and policy | Out of scope — must be named but not designed here |
| **Host Gem** | The kobako gem itself: Ruby API layer (`lib/`) + private native extension (`ext/`); exposes the sandbox interface, routes RPC, and manages Handle lifecycle | In scope |
| **Guest Binary** | `kobako.wasm` — compiled from the `wasm/` Rust crate; contains the mruby interpreter and RPC client; is the isolation boundary | In scope |
| **Service** | A Host Ruby object injected into the sandbox under a two-level name (`Namespace::Member`); the only mechanism by which guest code can access host resources | In scope |
| **Wire Spec** | The MessagePack contract governing all host↔guest RPC messages; not a runtime object but a shared protocol both sides implement | In scope |

**Key internal concepts** (refined in later layers):

- **Sandbox** (`Kobako::Sandbox`): the runtime unit that instantiates the Guest Binary, injects Services, executes a mruby script, and returns a structured outcome or raises a typed error.
- **Handle**: an opaque integer token the guest holds to reference a host-side object returned by a Service call; the guest can use it in subsequent RPC calls but cannot dereference it directly. Handle lifecycle is fully managed by the Host Gem; the guest holds only an opaque integer ID and cannot dereference it.
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
- Execute a mruby script synchronously and return its last expression as a deserialized Ruby value
- Route guest-initiated RPC calls to the correct host Service object and return the serialized result
- Represent stateful host-side objects returned by Service methods as opaque Capability Handles; allow the guest to reference those handles in subsequent calls
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
- The Host App supplies a valid mruby script string at call time
- Service objects provided by the Host App respond to whatever methods guest code will call; kobako does not validate Service shape
- The host machine has Rust/Cargo available to compile the native extension from source at gem install time
- Each `Kobako::Sandbox` instance is owned by a single Ruby Thread for the duration of any `#run`; concurrent `#run` invocations on the same Sandbox instance from multiple Threads are not supported. Distinct Sandbox instances may be used from distinct Threads (see B-22).

**Output guarantees:**
- Every `Sandbox#run` call either returns a single deserialized Ruby value (the script's last expression) or raises exactly one of `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError` — no other outcome is possible
- Guest stdout and stderr are always available as separate byte buffers after execution and contain no protocol bytes; truncation, when triggered by a configured cap, is observable via separate predicates on the Sandbox and never appears as inline content within the byte streams
- Capability state is fully reset between successive `#run` invocations on the same Sandbox instance
- The `kobako` gem name and the public Ruby class names `Kobako::Sandbox`, `Kobako::RPC::Handle`, `Kobako::RPC::Namespace`, `Kobako::TrapError`, `Kobako::SandboxError`, and `Kobako::ServiceError` are stable public contracts

#### Control — what kobako controls / depends on

**Controls:**
- The entire guest execution environment: mruby interpreter lifecycle, Wasm memory, and capability state
- Handle lifecycle — the guest holds only an opaque integer ID; the Host Gem owns the mapping from ID to host object and all allocation/deallocation decisions
- The host↔guest message codec: MessagePack encoding with two registered ext types (Capability Handle `0x01`, Fault envelope `0x02`)
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
| F-04 | Synchronous script execution | Host Gem + Guest Binary |
| F-05 | Guest-initiated RPC dispatch | Host Gem + Wire Spec |
| F-06 | Capability Handle encoding and referencing | Host Gem + Wire Spec |
| F-07 | Three-class error attribution and raising | Host Gem |
| F-08 | Guest output capture | Host Gem + Guest Binary |
| F-09 | Host–guest message codec | Wire Spec (both sides) |
| F-10 | Reproducible build pipeline | Build tooling |
| F-11 | Multi-layer test and benchmark suite | Quality pipeline |
| F-12 | Guest block reception and host-initiated yield re-entry | Host Gem + Guest Binary + Wire Spec |

---

### User Journeys

The following journeys describe the primary ways actors use kobako end-to-end. Each journey is a discrete, runnable scenario that covers one or more Impacts stated in Intent.

---

#### J-01 — LLM agent author runs model-generated code with curated capabilities

**Context**
An LLM agent framework author has a pipeline that feeds model-generated Ruby scripts to kobako at runtime. The Host App holds credentials (API keys, database connections); the generated scripts are untrusted and structurally unknown in advance. The author needs structured results back and must ensure no generated script can exfiltrate credentials or corrupt host state.

**Action**
1. The Host App creates a `Kobako::Sandbox` and declares Namespaces for the capabilities the generated scripts may legally call (e.g., a key-value lookup, a write-access log sink).
2. For each model-generated script, the Host App calls `Sandbox#run` with the script string, passing no additional configuration at call time.
3. The Host App reads the return value of `#run` as the structured result of the script's final expression.

**Outcome**
The Host App receives a deserialized Ruby value for every successful execution. Generated scripts that exceed their declared capabilities receive a `Kobako::ServiceError` (undefined member), scripts with Ruby errors raise `Kobako::SandboxError`, and Wasm-level failures raise `Kobako::TrapError` — the agent framework routes each class differently (retry, log, restart sandbox). At no point can a generated script read host memory or call methods not bound as Members.

---

#### J-02 — Host App developer integrates kobako into an existing service

**Context**
A Host App developer is adding kobako to a running Rails or Rack application for the first time. They need to understand the one-time configuration steps and the per-request execution contract before writing any business logic.

**Action**
1. The developer adds kobako to the project's gem dependencies and installs it; the native extension compiles from source.
2. The developer creates a `Kobako::Sandbox`, calls `define` to declare one or more Namespaces, and calls `bind` on each namespace to attach host objects as named Members.
3. At request time, the developer calls `Sandbox#run` with a script string and uses the return value as the execution result; they also read `Sandbox#stdout` and `Sandbox#stderr` for any guest log output.
4. The developer repeats step 3 for subsequent requests on the same Sandbox instance.

**Outcome**
The developer observes that the same Sandbox instance correctly resets capability state between `#run` calls — a Handle issued during one call is not reachable in the next. The Service objects bound at setup time remain active across all runs without re-registration. The developer can integrate kobako into request-handling middleware or background job workers using this setup-once / run-many pattern.

---

#### J-03 — Teaching platform operator evaluates student submissions in isolation

**Context**
A teaching platform or CI system operator receives student-submitted Ruby scripts for automated evaluation. Each submission must run in strict isolation: a failing or malicious submission must not affect the evaluation of any other submission, and no submission may access the host filesystem, network, or credentials.

**Action**
1. For each submission, the operator creates a fresh `Kobako::Sandbox`.
2. The operator optionally binds a grading Service that exposes read-only test fixtures and nothing else.
3. The operator calls `Sandbox#run` with the student's script string and collects the return value and `Sandbox#stdout` / `Sandbox#stderr` output.
4. The operator repeats this for each submission without restarting the host process.

**Outcome**
Each submission executes inside an isolated Wasm boundary. A submission that crashes or attempts to escape receives a `Kobako::TrapError` (or one of its subclasses) or `Kobako::SandboxError`; neither outcome affects subsequent submissions. Each Sandbox enforces a configurable per-run wall-clock timeout (default 60 s) and linear memory cap (default 5 MiB); submissions exceeding either raise `Kobako::TimeoutError` or `Kobako::MemoryLimitError` respectively, and never block the calling thread beyond the configured timeout. The Host App owns higher-level policy (queue-level fairness, per-student daily caps, retry semantics) above these per-run caps. The operator receives the script's result value and captured output for every submission that completes. No submission can read another submission's guest output or access host resources beyond the bound grading Service.

---

#### J-04 — No-code platform evaluates user-defined expressions per request

**Context**
A no-code or low-code platform builder allows end users to write Ruby expressions in formula fields or webhook filter rules. These expressions are evaluated on every incoming event or record. The platform needs sub-second evaluation latency, per-user capability scoping, and the guarantee that a broken user expression does not disrupt the platform's own process.

**Action**
1. The platform creates one `Kobako::Sandbox` per tenant, binding a Member that exposes the current record or event payload as a read-only object.
2. On each incoming event, the platform calls `Sandbox#run` with the user's expression string.
3. The platform reads the return value as the expression result and uses it to drive downstream logic (filter pass/fail, computed field value).

**Outcome**
User expressions that produce a valid Ruby value return it as a deserialized result. Expressions with syntax or runtime errors raise `Kobako::SandboxError`, which the platform surfaces to the user as an expression error without disrupting other tenants. Because each Sandbox's state fully resets between `#run` calls, a user cannot accumulate state across evaluations. Subsequent evaluations on the same Sandbox instance do not incur the cold-start cost of the first execution.

---

#### J-05 — Host App developer distinguishes and handles the three error classes

**Context**
A Host App developer is adding error handling to an existing kobako integration. They need to respond differently to execution failures depending on whether the failure originates in the Wasm engine, the sandboxed script itself, or a Service call made by the script.

**Action**
1. The developer wraps `Sandbox#run` in a rescue block that catches `Kobako::TrapError`, `Kobako::SandboxError`, and `Kobako::ServiceError` as separate branches.
2. For `TrapError`, the developer logs the failure and recreates the Sandbox before the next execution.
3. For `SandboxError`, the developer records the error as a script-level fault (wrong script, not broken infrastructure) and surfaces it to the script's author.
4. For `ServiceError`, the developer treats it as a capability-level fault (the script called a Service correctly but the Service reported an error) and applies the same retry or alerting policy as any other service failure in the Host App.

**Outcome**
The developer can route each failure class through the Host App's existing error-handling infrastructure without inspecting error messages. The three-class taxonomy gives the developer a reliable signal for triage: infrastructure fault (TrapError), authored-code fault (SandboxError), or downstream-service fault (ServiceError). This attribution is guaranteed by kobako regardless of what the guest script does.

---

#### J-06 — Host App exposes a block-yielding Service

**Context**
A Host App developer is building a Service that iterates over a collection on the host side and wants each element to be processed by a guest-supplied block (similar to `Array#each` semantics). The Service's natural Ruby form takes a block; the developer wants guest scripts to call it as `MyEach.run(items) { |x| ... }` without learning a different API for the sandboxed environment.

**Action**
1. The developer defines a Ruby class whose method accepts a block (`def run(items, &blk); items.each { |x| yield x }; end`) and binds an instance under a Namespace Member.
2. A guest script writes `Service::MyEach.run([1, 2, 3]) { |x| x * 2 }` — the block is part of the script, not part of the host code.
3. The Host App calls `Sandbox#run` with this script and reads the return value.

**Outcome**
The Service method's `yield x` invokes the guest block once per iteration, returning each block result back to the host method as the value of `yield`. The Service method observes its block as an ordinary Ruby Proc with loose arity; the guest-side block executes inside the Wasm sandbox and remains isolated from host state. A `break` inside the guest block terminates the Service method early with the break value, matching standard Ruby semantics. A `next` (or natural fall-through) returns the block value to `yield` and execution continues. Exceptions raised inside the block propagate to the `yield` point where the Service method may rescue or let them flow up. The developer writes the Service in idiomatic Ruby; the sandbox boundary is invisible from the Service method's perspective.

---

## Behavior

The per-anchor behavior table (Initial State → Operation → Result / Final State) for B-01..B-30 and the Error Scenarios subsection covering E-01..E-23 are specified in detail in [`docs/behavior.md`](docs/behavior.md). The decisions below govern those behaviors; consult the linked document for each anchor's full Initial State / Operation / Result / Notes.

- **Four-outcome guarantee:** every `Sandbox#run` invocation terminates in exactly one of — a return value, `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError`. No partial completion, no other outcome.
- **Attribution is two-step:** Step 1 — if the Wasm engine reports a trap (including configured-cap traps), raise `Kobako::TrapError` or its named subclass (`Kobako::TimeoutError` per E-19, `Kobako::MemoryLimitError` per E-20). Step 2 — otherwise dispatch on the outcome envelope first-byte tag (`0x01` result, `0x02` panic). Zero-length outcome bytes or unknown tags raise `Kobako::TrapError` as wire-violation fallback.
- **`stdout` / `stderr` never participate in attribution.** They are captured separately and remain readable after error-raising runs.
- **Setup-time errors raise `ArgumentError`, not `SandboxError`:** invalid Namespace / MemberName patterns (E-16, E-17) and `define`-after-`#run` (E-18) are Host App programming errors detected before or between guest executions; they bypass the attribution pipeline.
- **Anchor groupings:** B-01..B-06 cover Sandbox construction, per-run lifecycle, and output capture; B-07..B-11 cover Namespace / Member registration; B-12..B-21 cover guest-initiated RPC dispatch and HandleTable lifecycle; B-22 covers per-Thread isolation; B-23..B-30 cover Block / Yield re-entry. Errors split across the three classes — `TrapError` (E-01..E-03, E-19, E-20), `SandboxError` (E-04..E-10, E-16..E-18, E-21..E-23), and `ServiceError` (E-11..E-15).

---

## Refinement

`B-xx` and `E-xx` anchors referenced throughout this layer are defined in detail in [`docs/behavior.md`](docs/behavior.md) per Naming Principle N-8.

### Terminology

This section defines every term used in this specification. Each concept has one primary canonical name. Documented aliases are permitted only when both names appear together in this section with the canonical relationship explicitly stated.

---

#### Roles

These five roles describe every actor and artifact in the system. All sections of this specification use these names exclusively.

| Term | Definition | Layer |
|------|-----------|-------|
| **Host App** | The Ruby application (Rails / Rack / Sidekiq / CLI) that uses kobako; holds all credentials, policy, and Service objects. Out of scope for design but referenced throughout. | External |
| **Host Gem** | The `kobako` gem itself: the Ruby API layer (`lib/`) plus the private native extension (`ext/`). Exposes the sandbox interface to the Host App, routes RPC calls, and manages Handle lifecycle. | In scope |
| **Guest Binary** | The file `kobako.wasm`, compiled from the `wasm/` Rust crate. Contains the mruby interpreter and RPC client. Is the isolation boundary between host and guest execution environments. | In scope |
| **Service** | A Host Ruby object injected into the sandbox under a two-level name (`Namespace::Member`). The only mechanism by which guest code can access host resources. | In scope |
| **Wire Spec** | The MessagePack contract governing all host↔guest RPC messages. Not a runtime object — it is a protocol that both Host Gem and Guest Binary implement independently. | In scope |

*Layer values: **In scope** — designed in this specification; **External** — outside this design, referenced for contract completeness.*

---

#### Internal Concepts

These are sub-components and runtime concepts internal to kobako. They are not exposed as a public API to the Host App unless explicitly stated. The **Server** / **Client** pair forms the host↔guest **RPC** roles: Server lives in the Host Gem and routes inbound calls; Client lives in the Guest Binary and initiates outbound calls.

| Term | Definition | Public? |
|------|-----------|---------|
| **Sandbox** | The runtime unit instantiated by `Kobako::Sandbox`. Owns the Guest Binary lifecycle, RPC Server, HandleTable, and output buffers for a single logical execution context. Enforces three configurable per-run caps — wall-clock timeout, linear memory cap, and per-channel output cap — each independently disableable with `nil`. Maps to the Ruby class `Kobako::Sandbox`. | Yes — `Kobako::Sandbox` is stable public API |
| **Server** | The host-side RPC coordinator. Maintains Namespace / Member registrations, routes incoming Requests to the resolved Service object, and owns the HandleTable. Maps to Ruby class `Kobako::RPC::Server`. Not exposed to the Host App. | No |
| **Client** | The base mruby class representing a remote Member inside the Guest Binary. Maps to mruby class `Kobako::RPC::Client`. Guest scripts do not reference Client directly; they see module constants under their declared Namespace. All method calls on a Client are forwarded as Requests to the host Server. Internal to the Guest Binary; not visible to the Host App or guest scripts as a named class. | No |
| **HandleTable** | The host-side mapping from Handle IDs to Ruby objects. Owned by the Server. Created fresh at the start of each `#run` and fully discarded at the end. Not exposed to the Host App. | No |
| **Handle** | An opaque integer token the guest holds to reference a host-side object returned by a Service call. The guest can pass it as an RPC target or argument in subsequent calls but cannot dereference it to a Ruby value. Maps to two independent implementations with the same canonical name: the Ruby class `Kobako::RPC::Handle` runs in the host process; the `Kobako::RPC::Handle` mruby class runs inside the Wasm guest. They share neither code nor instances. | Partially — `Kobako::RPC::Handle` instances may surface as fields on raised `SandboxError` or `ServiceError` instances; the Host App has no public constructor or inspection methods |
| **Capability Handle** | A Handle that represents a stateful host-side resource (e.g., a session, connection, or any object that is not a primitive wire type). Transmitted on the wire as MessagePack ext type `0x01`. "Capability Handle" is used when emphasizing the capability-granting semantics; "Handle" is used for brevity elsewhere — both refer to the same concept. | No — same visibility as Handle; no distinct class exists |
| **Block** | A mruby block (or Proc/lambda) the guest passes alongside a Service method call. The block body lives inside the Guest Binary and is never serialized; only its presence is signalled on the wire (Request `has_block` field). Scoped to the single dispatch call that received it — not reusable after that dispatch returns. | No — surfaces only as the `&block` argument the Service method receives |
| **Yield** | A single synchronous round-trip from a Service method into the Block it received. The host Service method invokes `yield` or `block.call` on its block argument; the Host Gem re-enters the Guest Binary, executes the block body, and returns the block's result to the host yield site. Each `yield` is an independent round-trip; a Service method may yield zero or more times during a single dispatch. | No |
| **Yield Proxy** | The host-side Ruby Proc the Host Gem materialises to represent the guest block to the Service method. Has loose Proc-style arity (matches `&block` Ruby conventions). Valid only for the duration of the dispatch that produced it; invocation outside that scope raises (E-23). The Service method may invoke it directly via `block.call` or implicitly via `yield`. | No |

---

#### Error Classes

Three error classes cover every failure outcome of `Sandbox#run`. These class names are stable public API and must not be renamed or aliased.

| Term | Ruby Class | Layer it represents | When raised |
|------|-----------|--------------------|----|
| **TrapError** | `Kobako::TrapError` | Wasm engine layer | The Wasm execution engine crashed (OOM, `unreachable`, stack overflow, import signature mismatch) or the wire layer detected a structural violation indicating a corrupted guest runtime (zero-length outcome, unknown outcome tag) |
| **SandboxError** | `Kobako::SandboxError` | Sandbox / wire layer | The guest ran to completion but execution failed due to a protocol fault, a mruby runtime error, or a host-side wire decode failure. The Wasm instance is retired normally; the sandbox infrastructure is intact |
| **ServiceError** | `Kobako::ServiceError` | Service / capability layer | The guest ran to completion, the mruby script itself did not crash, but a Service capability call reported an application-level failure that was not rescued within the script |

**Named subclasses (stable public API):**

| Term | Ruby Class | Superclass | Meaning |
|------|-----------|-----------|---------|
| **TimeoutError** | `Kobako::TimeoutError` | `Kobako::TrapError` | Absolute wall-clock time since `Sandbox#run` invocation reached the configured per-run `timeout` (default 60 s); trap fires at the next guest wasm safepoint after the deadline. See E-19; B-01 Notes covers host-callback accounting. |
| **MemoryLimitError** | `Kobako::MemoryLimitError` | `Kobako::TrapError` | Guest `memory.grow` would exceed the configured per-run `memory_limit` (default 5 MiB); see E-20 |
| **HandleTableExhausted** | `Kobako::HandleTableExhausted` | `Kobako::SandboxError` | Handle ID counter reached `0x7fff_ffff` (2³¹ − 1) within a single `#run`; further allocation is impossible |
| **ServiceError::Disconnected** | `Kobako::ServiceError::Disconnected` | `Kobako::ServiceError` | RPC target Handle resolves to the `:disconnected` sentinel in the HandleTable |

**Wire-level error string (not a Ruby class):** The string `"Kobako::RPC::WireError"` appears only as the `class` field value in a Panic envelope (defined in [`docs/wire-contract.md`](docs/wire-contract.md) § Outcome Envelope; the governing summary lives below in `### Wire Contract`) to signal that the wire layer detected a violation. On the host side this maps to a raised `Kobako::SandboxError`; there is no standalone `Kobako::RPC::WireError` Ruby class on the host. (The guest mruby class `Kobako::RPC::WireError` exists only to be raised inside the Guest Binary; it is captured by the guest's top-level handler and converted into the panic envelope string.)

---

#### Namespace and Member

These terms describe the two-level injection model used to expose host capabilities to guest scripts.

| Term | Definition | Guest-visible form |
|------|-----------|-------------------|
| **Namespace** | A named grouping declared by the Host App via `sandbox.define(:Name)`. Namespaces are declared at setup time before the first `#run`. The namespace itself holds no state — it is a container for Members. Maps to Ruby class `Kobako::RPC::Namespace`. | Ruby module (e.g., `MyService`) |
| **Member** | A Host Ruby object bound into a Namespace via `namespace.bind(:Name, object)`. The Member is the object that receives RPC calls dispatched from guest scripts. | Module constant (e.g., `MyService::KV`) |

---

### Wire Contract

The abstract logical shape of every host↔guest message exchanged during a `#run` invocation is specified in detail in [`docs/wire-contract.md`](docs/wire-contract.md). It is a Consistency-layer contract: both sides implement it independently, and a kobako gem release ships exactly one version of it. Byte-level encoding (msgpack type mapping, ext code numbers, binary layout) is specified in [`docs/wire-codec.md`](docs/wire-codec.md). The decisions below govern the contract; the linked documents reproduce them as field tables, envelope shapes, and per-position rules.

- **Transport role:** the Guest Binary is the sole initiator of host↔guest communication; the Host Gem responds synchronously within the same Wasm import call frame. There are no callbacks, promises, or yield-resume mechanisms.
- **Request shape:** every Request carries exactly five logical fields — `target` (Member path or Capability Handle reference, distinguishable at the first wire byte), `method` (string), `args` (ordered list, may include Handle references), `kwargs` (Symbol-keyed map; empty kwargs is always present, never absent), and `has_block` (bool indicating the guest call site supplied a block; the block body is never serialized — only the flag travels).
- **Response shape:** mutually exclusive Success (`status=0`, `value`) or Fault (`status=1`, fault envelope) variant. No partial success, no streaming.
- **Capability Handle contract:** opaque to the guest (cannot be dereferenced or constructed from a bare integer); host-allocated when a Service returns a stateful object; scoped to a single `#run` (HandleTable reset at every `#run`); ID bounded by `0x7fff_ffff` (allocation beyond raises `Kobako::SandboxError`, no silent wraparound).
- **Fault envelope:** `type` (one of four reserved values — `"runtime"`, `"argument"`, `"disconnected"`, `"undefined"` — stable across releases), `message` (string), `details` (optional structured data). Adding a new `type` requires a coupled host+guest release.
- **Outcome envelope:** per-`#run` final result, distinct from per-RPC Response; two variants — Result envelope (serialized last mruby expression) or Panic envelope (`origin` / `class` / `message` / `backtrace`, where `origin="service"` attributes to `Kobako::ServiceError` and `origin="sandbox"` or absent attributes to `Kobako::SandboxError`). Zero-length outcome bytes or an unrecognized tag raises `Kobako::TrapError`.
- **Yield round-trip:** the host-initiated counterpart of a Request/Response. The Host Gem re-enters the guest synchronously when a Service method invokes its yield proxy (B-24); yield round-trips nest strictly within the dispatch frame that produced the proxy and stack in LIFO order; each frame holds at most one proxy.
- **YieldResponse envelope:** three live tag values — `0x01` ok (block returned a wire-legal value), `0x02` break (block executed `break val` from a non-lambda context, unwinds the Service method per B-25), `0x04` error (block raised, returned an unrepresentable value, used `return` from a non-lambda block, or invoked an escaped yield proxy — E-21..E-23). `0x03` is reserved and rejected as a wire violation.
- **Release-internal contract:** the Wire Spec ships in a single kobako gem release that updates both sides simultaneously. No in-band version field, no negotiation mechanism, no one-sided evolution; the single wire shape defined in the release is the only shape either side implements. Wire-affecting changes are recorded in `CHANGELOG.md` under Breaking Changes.

---

### Wire Codec

The byte-level codec that pins the Wire Contract is specified in detail in [`docs/wire-codec.md`](docs/wire-codec.md). The decisions below govern the codec; the linked document reproduces them as binary layouts, hex examples, and per-position rules.

- **Codec:** MessagePack on both sides of the Wasm boundary; no fallback or alternative codec is permitted. All Requests, Responses, and Outcome envelopes are MessagePack-encoded byte sequences.
- **Wire type set:** exactly 12 entries — `nil`, `bool`, `int`, `float`, `str`, `bin`, `array`, `map`, and the three ext codes `0x00` (Symbol), `0x01` (Capability Handle), `0x02` (Fault Envelope). Any msgpack type or ext code outside this set is a wire violation; both sides reject without attempting to decode.
- **Ext code assignments are stable for the life of a release:** `0x00` Symbol (kwargs map keys MUST be this type), `0x01` Capability Handle (`fixext 4`, big-endian u32 ID, cap `0x7fff_ffff`), `0x02` Fault Envelope (embedded msgpack map carrying `type` / `message` / `details`).
- **Envelope framing:** Request is a 5-element msgpack array (`target`, `method`, `args`, `kwargs`, `has_block`); Response is a 2-element msgpack array (`status`, `value`-or-fault); Result envelope emits the value directly (Outcome tag discriminates); Panic envelope is a msgpack map (keys `origin`, `class`, `message`, `backtrace`, `details`); Outcome envelope is a one-byte tag (`0x01` result / `0x02` panic) followed by its payload; YieldResponse envelope is a one-byte tag (`0x01` ok / `0x02` break / `0x04` error; `0x03` reserved) followed by an optional payload.
- **ABI surface:** one host import (`__kobako_dispatch`) and four guest exports (`__kobako_run`, `__kobako_alloc`, `__kobako_take_outcome`, `__kobako_yield_to_block`); function names and Wasm signatures are fixed across a release. `__kobako_dispatch`, `__kobako_take_outcome`, and `__kobako_yield_to_block` return a packed i64 carrying `ptr` (high 32 bits) and `len` (low 32 bits).
- **Single-RPC payload cap:** 16 MiB in either direction. Exceeding the cap is a wire violation; the Host Gem walks the trap path.
- **Consistency guarantee:** round-trip fuzz between the two independently-implemented codecs (Ruby host, Rust/mruby guest) is the sole correctness mechanism, covering all 12 wire types and all 3 ext types in both directions. Any round-trip failure is a wire regression that blocks release. The harness contract is specified in Implementation Standards § Testing Style.

---

### Naming Principles

The following principles govern how all names in this specification and in the `kobako` public surface are formed. They are declarative rules, not rationale.

| # | Principle | Applies to |
|---|----------|-----------|
| N-1 | Role names are PascalCase with every word capitalized: `Host App`, `Host Gem`, `Guest Binary`, `Wire Spec` | All role names in this document and in code comments |
| N-2 | All public Ruby classes and modules live under the `Kobako::` namespace | Ruby classes: `Kobako::Sandbox`, `Kobako::TrapError`, `Kobako::SandboxError`, `Kobako::ServiceError`, `Kobako::RPC::Handle`, `Kobako::RPC::Namespace` |
| N-3 | The gem name is always lowercase: `kobako` | Gemspec, `require` statements, Bundler references |
| N-4 | The Wasm artifact name is fixed: `kobako.wasm` | Build output, gem packaging, documentation |
| N-5 | Internal Rust crates are named with a hyphen prefix matching the gem: `kobako-wasm` (Guest Binary crate), `kobako-ext` (native extension crate) | `Cargo.toml` package names; not exposed to Ruby |
| N-6 | A concept has exactly one name; no synonyms appear in the same document or public surface | All layers of this specification |
| N-7 | Error class names encode the layer they represent: `TrapError` → Wasm engine layer, `SandboxError` → sandbox/wire layer, `ServiceError` → service/capability layer | `Kobako::TrapError`, `Kobako::SandboxError`, `Kobako::ServiceError` |
| N-8 | `B-xx` and `E-xx` anchors are assigned monotonically and append-only across the spec corpus (`SPEC.md` plus `docs/*.md`); existing anchors are never renumbered, and a new entry takes the next unused number regardless of which document or subsection it belongs to. Relocation of a contiguous anchor range from `SPEC.md` to `docs/<topic>.md` during a TOC-mode extraction is not a renumbering and is permitted | All Behavior and Error Scenario entries |

---

### Implementation Standards

#### Architecture

The kobako codebase is split into two top-level source areas with a strict boundary between them:

- **`lib/`** — the Host Gem Ruby surface. Contains `kobako.rb` (the main entry point that loads the native extension and defines the public API) and `lib/kobako/` sub-modules (error class definitions, codec helpers, RPC value objects, and the RPC Server). This is the only layer the Host App interacts with directly.
- **`ext/kobako/`** — the private native extension (`kobako-ext` Rust crate). Wraps wasmtime, owns the Wasm engine lifecycle, and implements the host-side import function `__kobako_dispatch`. This is a private implementation detail of the Host Gem; it is never intended as a reusable wasmtime binding and exposes no Wasm engine types to the Host App or downstream gems.
- **`wasm/`** — the Guest Binary source (`kobako-wasm` Rust crate, target `wasm32-wasip1`). This is build-time only; it is compiled to `data/kobako.wasm` and excluded from the published gem alongside build tools (`vendor/`, `tasks/`, `build_config/`).
- **`data/kobako.wasm`** — the pre-built Guest Binary artifact. Produced at release time on the publisher's machine and shipped inside the gem. End users receive this file at install time; they never need to recompile the Wasm side.

The boundary rule is: **`ext/` is private to the Host Gem and must never be imported by downstream gems**; `lib/` is the stable public surface. The host-side build (`ext/`) and the guest-side build (`wasm/`) maintain independent Cargo workspaces and separate lock graphs. The root `Cargo.toml` contains only `ext/kobako` in `members` and excludes `wasm/` and `vendor/` — this isolation prevents host-only crates (e.g., `wasmtime`) from appearing in the wasm32 dependency graph.

#### Design Patterns

The following patterns are enforced project-wide and apply at every layer:

- **Wire is a release-internal contract** — see `### Wire Contract` § release-internal contract for the governing statement. Design implication: never add a wire field gated on a flag both sides do not compile in the same release; treat the Wire Spec as a single coupled artifact.
- **Round-trip fuzz is the consistency guarantee.** The host-side codec is implemented in pure Ruby under `lib/kobako/codec/` and is loadable at `require` time before the native extension is available; the guest-side codec is implemented in Rust under `wasm/kobako-wasm/src/codec/` for the `wasm32-wasip1` target. The two implementations share no source code — the deployment model (the gem must `require` cleanly without a built native extension, and `wasm32-wasip1` cannot embed Ruby) forbids a single codec. Correctness is established by bidirectional round-trip fuzz covering all 12 wire types and all three ext types.
- **Codec depends on RPC value objects.** The Codec layer registers `Kobako::RPC::Handle` as its ext 0x01 decode target and `Kobako::RPC::Fault` as its ext 0x02 decode target. The dependency direction is Codec → RPC value objects; the RPC layer does not depend on Codec. This makes RPC value objects loadable without the codec available and keeps the codec a pure transformation over a known set of host-side types.
- **Three-layer error attribution is two-step** — see `## Behavior` § attribution for the governing Step 1 / Step 2 decision. Design implication: error classification is a pure function of `(trap?, outcome_tag)`; exit codes, stdout, and stderr are never inputs to that function at either step.
- **Source-only distribution.** The published gem does not include precompiled native extensions for any platform. End users compile `ext/kobako/` from Rust source using their local Rust toolchain and cargo. The only pre-built binary artifact shipped in the gem is `data/kobako.wasm`.
- **Build-time vendor isolation.** `vendor/wasi-sdk/` and `vendor/mruby/` are fetched from official release tarballs at build time and are never committed to the repository. Version numbers are pinned as constants inside `tasks/vendor.rake`. This avoids git submodule pointer maintenance and guarantees cross-environment reproducibility.
- **Fix the bottom layer, not the top.** When a gap is found in a low-level interface (codec type coverage, setjmp/longjmp flag, Wire Spec field, HandleTable guard, Panic envelope schema), the fix is applied to the interface layer itself. Working around a low-level gap in a higher-level capability or application layer is not permitted.
- **Process-scope Engine and Module cache.** The wasmtime Engine and the compiled Module for `data/kobako.wasm` are cached at process scope by the native extension. The first `Kobako::Sandbox` constructed in a process pays Engine init and Module compile; every subsequent Sandbox in the same process — regardless of which Thread constructs it — amortizes against this shared state. The cache is implicit; the Host App has no API to inspect, warm, or invalidate it. This pattern is what makes the Sandbox-per-tenant and Sandbox-per-Thread shapes (B-22) practical.

##### Invariants

The following invariants hold across every layer of the system. Each is a hard rule; no layer may violate them.

| Invariant | Applies to | Enforcement |
|-----------|-----------|-------------|
| The terms `Namespace` and `Member` (not "tool" or generic names) are used everywhere in code, documentation, and wire values | All layers | Documentation |
| Wire `target` for RPC calls uses the Ruby constant-path form `"Namespace::Member"`; Handle references use ext 0x01 — both forms are distinguishable at the first wire byte | Wire Spec, both codec implementations | Test-time |
| Error attribution is determined solely by `(trap?, outcome_tag)` — stdout, stderr, and exit codes are excluded from attribution logic | Host Gem, error handling | Test-time |
| stdout and stderr carry only user-observable guest output; no kobako protocol bytes appear on these channels | Guest Binary, Host Gem | Test-time |
| `#stdout` and `#stderr` byte content never includes truncation sentinels; truncation status is observable only via `#stdout_truncated?` / `#stderr_truncated?` | Host Gem | Test-time |
| `#run` exceeding the configured `timeout` raises `Kobako::TimeoutError` via the trap-attribution path; no other outcome is possible for wall-clock cap exhaustion | Host Gem | Runtime |
| Guest `memory.grow` exceeding the configured `memory_limit` traps unconditionally and raises `Kobako::MemoryLimitError`; the host never observes a silent `memory.grow` failure from cap exhaustion | Host Gem | Runtime |
| `Sandbox#run` returns the last mruby expression value via the Result envelope path; objects without a wire representation take the Panic envelope path — no implicit `inspect` or `to_h` conversion | Guest Binary, Wire Spec | Test-time |
| `vendor/` is never committed to the repository; build tools fetch release tarballs at build time | Repository, task scripts | Build-time |
| mruby exception unwind is implemented via wasi-sdk setjmp/longjmp (three mandatory compiler flags); direct modification of mruby setjmp call sites is not permitted | Guest Binary build | Build-time |
| Guest Binary target is `wasm32-wasip1`; wasi-preview2 and component model are out of scope | Guest Binary build, Host Gem | Build-time |
| HandleTable IDs are bounded by `0x7fff_ffff` (2³¹ − 1); exceeding the cap raises `Kobako::SandboxError` immediately — no silent wraparound or truncation | Host Gem, wire layer | Runtime |
| `ext/kobako/` is a private binding for the kobako gem only; no downstream gem may depend on it directly | Architecture | Documentation |
| Handle lifecycle is per-`#run`: the HandleTable is fully cleared and the counter reset to 1 at the start of every `#run`; Handles from run N are invalid in run N+1 | Host Gem, Wire Spec | Test-time |
| Handles are never individually released by the guest; the host implementation does not use `ObjectSpace.define_finalizer` for HandleTable entries | Host Gem | Documentation |
| Wire ABI has exactly one host import (`__kobako_dispatch`) and four guest exports (`__kobako_run`, `__kobako_alloc`, `__kobako_take_outcome`, `__kobako_yield_to_block`); no additional imports or exports are permitted | Wire Spec, both codec implementations | Build-time |
| Yield round-trip nests strictly within the dispatch frame whose Service method initiated it; nested dispatch frames each receive at most one yield proxy and the proxies stack in LIFO order — they are not interchangeable across frames | Wire Spec, Host Gem | Runtime |
| Guest mruby's `MRB_STR_LENGTH_MAX` is 1 MiB — a guest-side String at or above this size raises `ArgumentError` inside the guest. This is independent of the 16 MiB single-RPC wire payload limit; a wire payload can approach the 16 MiB cap via composite values (Array, binary), but a single guest String value cannot. | Guest Binary build (mruby config) | Runtime |

#### Testing Style

The test suite is organized into four layers. All four layers must exist and must pass before a release is approved. No single layer may substitute for another.

| Layer | Name | Scope | When it must pass |
|-------|------|-------|------------------|
| 1 | **Codec round-trip fuzz** | Bidirectional wire codec agreement between Host Gem and Guest Binary codec implementations; covers all 12 wire types, all three ext types, and nested compositions | Always — any failure is a wire regression that blocks release unconditionally |
| 2 | **Wire integration** | Full Request / Response exchange through a live Sandbox, including the disconnected sentinel path and all envelope type variants | Before release |
| 3 | **Ext unit** | `ext/kobako/` internal Rust unit tests and `lib/kobako/` Ruby specs without starting a Sandbox; includes HandleTable allocation / release / fetch, `HandleTableExhausted` guard at `0x7fff_ffff`, wire encode/decode boundary values, and wasmtime API wrapper correctness | Before release; the HandleTable exhaustion guard is also a required build-pipeline guard (see below) |
| 4 | **End-to-end** | Full Host App → `Sandbox#run` → Service call → result return path; must cover all three error attribution paths (`TrapError`, `SandboxError`, `ServiceError`) with each trigger, kwargs dispatch (including empty kwargs, symbol-key wire form, and Symbol round-trip through args / return values), Handle chaining (Service returns stateful object, guest uses Handle as subsequent RPC target), Handle lifecycle over Sandbox teardown, cross-run Handle invalidity (a Handle obtained in run N used as a target in run N+1 surfaces as `Kobako::ServiceError` with `type="undefined"` when not rescued within the script — see B-18, E-13), block / yield round-trip (Service method receives a block via `&block` and yields one or more times; covers each YieldResponse tag — `0x01` ok, `0x02` break with B-25 unwind semantics, `0x04` error from block exception, and the unsupported-`return` path of E-21 raising at the yield site; covers lambda-block `break` silent return per B-27 and nested dispatch frames per B-28), stdout / stderr isolation from the protocol channel, and the wire-violation edge cases (`len=0`, unknown tag, Result envelope with unrepresentable value) | Before release |

The recommended execution order is Layer 3 → Layer 1 → Layer 2 → Layer 4 (cheapest first; fail fast before starting the Sandbox).

**Layer 1 harness contract** — the Codec round-trip fuzz harness must satisfy two cross-implementer requirements regardless of transport mechanism (in-process FFI, subprocess IPC, or wasmtime-embedded invocation):

- The random seed for each run is sourced from an environment variable, and any failing iteration's failure output includes the seed in use; a failing run is reproducible from the seed alone.
- The generator records which wire types and ext types it exercises; at the end of each run, the harness asserts that all 12 wire types and all three ext types were observed at least once. A coverage gap fails the harness independently of any byte-equality failure.

Iteration count and the transport between the two codec implementations are implementer-chosen.

**Build-pipeline guards** — the following checks must run as part of the build step, before the full test suite:

- HandleTable ID cap guard: after `ext/kobako/` is compiled, immediately verify that ID `0x7fff_ffff` is successfully allocated and that the next attempt raises `Kobako::HandleTableExhausted`.
- Gemspec files whitelist check: after `gem build kobako.gemspec`, verify that the resulting archive does not contain `vendor/`, `wasm/`, `tasks/`, or `build_config/` content.

**Regression benchmarks** — the following five benchmarks must be maintained in `benchmark/` with baseline results stored in git. Each release compares against the previous baseline; a regression greater than +10% requires explicit review and approval before release proceeds.

| # | Benchmark | What it detects |
|---|-----------|----------------|
| 1 | Cold start latency (`Kobako::Sandbox.new` → first `#run`) | wasmtime Module load / Engine initialization regression |
| 2 | RPC round-trip latency (single minimal Service call) | Wire codec, import function dispatch, HandleTable lookup combined |
| 3 | Codec throughput at varying payload sizes and nesting depths (host and guest sides measured separately) | Unnecessary allocations or codec path regressions |
| 4 | mruby script evaluation time (fixed script, no RPC) | Impact of `build_config/wasi.rb` flag changes on VM execution speed |
| 5 | Handle allocation and release throughput (bulk Service return value wrapping) | HandleTable internal dictionary and counter performance |

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
| 5 | `CHANGELOG.md` | Keep a Changelog format; each release includes Added / Changed / Fixed / Breaking Changes sections (empty sections may be omitted) |
| 6 | `LICENSE` | License file |

Wire-affecting changes that break round-trip compatibility are recorded in `CHANGELOG.md` under the Breaking Changes section. MSRV changes are treated as breaking changes and must appear in `CHANGELOG.md`.
