# SPEC.md — kobako

> This document is the authoritative specification for the kobako gem.
> It is written in progressive layers. This file currently contains the **Intent** and **Scope** layers.
> Behavior and Refinement layers will be appended in subsequent cycles.

---

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

When kobako is working correctly:

- A Host App can execute arbitrary mruby code supplied at runtime and receive a structured result or a categorized error — without any guest code affecting host memory, I/O, or credentials.
- A Host App can inject named Ruby service objects that guest code can call via RPC; those objects are the only mechanism by which guest code can interact with external resources.
- Errors produced during guest execution are attributable to one of three distinct origins (Wasm trap, sandbox/wire fault, or guest application error), enabling the Host App to handle each case differently.
- Guest stdout and stderr are captured and exposed separately from the RPC protocol channel, allowing Host Apps to surface guest logs without confusing them with protocol messages.

### Non-Goals

The following are explicitly outside the scope of kobako:

- LLM integration, agent frameworks, or prompt engineering (kobako provides the execution substrate; connecting it to an LLM is the Host App's responsibility)
- A general-purpose wasmtime Ruby gem — `ext/` is a private binding, not a reusable wasmtime wrapper
- mruby upstream development or distribution — kobako consumes a pinned mruby release tarball
- Multi-tenant billing, SLA management, or deployment/operations tooling
- Async / yield-resume execution models or interpreter state snapshot/resume (kobako uses synchronous blocking RPC; Monty-style `start()`/`resume()` and `dump()`/`load_snapshot()` are not provided)
- Passing guest-side blocks to Service methods — guest closures cannot be executed on the host side; iteration is handled by returning collections from Service methods

### Core Abstractions

These five roles describe the system. All design and behavior content in later layers uses these names exclusively.

| Role | Responsibility | Scope |
|------|---------------|-------|
| **Host App** | The Ruby application (Rails / Rack / CLI) that uses kobako; holds all credentials and policy | Out of scope — must be named but not designed here |
| **Host Gem** | The kobako gem itself: Ruby API layer (`lib/`) + private native extension (`ext/`); exposes the sandbox interface, routes RPC, and manages Handle lifecycle | In scope |
| **Guest Binary** | `kobako.wasm` — compiled from the `wasm/` Rust crate; contains the mruby interpreter and RPC client; is the isolation boundary | In scope |
| **Service** | A Host Ruby object injected into the sandbox under a two-level name (`Group::Member`); the only mechanism by which guest code can access host resources | In scope |
| **Wire Spec** | The MessagePack contract governing all host↔guest RPC messages; not a runtime object but a shared protocol both sides implement | In scope |

**Key internal concepts** (refined in later layers):

- **Sandbox** (`Kobako::Sandbox`): the runtime unit that instantiates the Guest Binary, injects Services, executes a mruby script, and returns a structured outcome or raises a typed error.
- **Registry**: the Host Gem sub-component that maintains Service Group / Member registrations, routes incoming RPC calls to the correct host object, and owns the HandleTable.
- **Handle**: an opaque integer token the guest holds to reference a host-side object returned by a Service call; the guest can use it in subsequent RPC calls but cannot dereference it directly.
- **HandleTable**: the host-side mapping from Handle IDs to Ruby objects; owned by Registry and not exposed to Host App.
- **Service Group / Member**: `Group` is a declared namespace visible to guest as a Ruby module; `Member` is a named binding within the group visible to guest as a module constant.
- **Three-layer error taxonomy**: `Kobako::TrapError` (Wasm trap), `Kobako::SandboxError` (wire or runtime fault), `Kobako::ServiceError` (guest application error) — each with distinct attribution and handling semantics.

---

<!-- Scope layer: append here -->

---

## Scope

### System Boundary

#### Responsibility — what kobako does / does not do

**Does:**
- Provide an in-process mruby execution environment isolated by a Wasm boundary
- Expose a Ruby API for Host Apps to declare Service namespaces and bind host objects as callable members
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
- Async or yield-resume execution — all execution is synchronous and blocking; snapshot/resume is not provided
- Guest-side closure invocation on the host — guest blocks cannot be passed to Service methods; iteration is handled by returning collections
- Multi-tenant billing, SLA management, deployment, or operational tooling
- Windows platform support — Linux and macOS only

#### Interaction — input assumptions / output guarantees

**Input assumptions:**
- The Host App supplies a valid mruby script string at call time
- Service objects provided by the Host App respond to whatever methods guest code will call; kobako does not validate Service shape
- The host machine has Rust/Cargo available to compile the native extension from source at gem install time

**Output guarantees:**
- Every `Sandbox#run` call either returns a single deserialized Ruby value (the script's last expression) or raises exactly one of `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError` — no other outcome is possible
- Guest stdout and stderr are always available as separate byte buffers after execution and contain no protocol bytes
- Capability state is fully reset between successive `#run` invocations on the same Sandbox instance
- The `kobako` gem name and the public Ruby class names `Kobako::Sandbox`, `Kobako::Handle`, `Kobako::TrapError`, `Kobako::SandboxError`, and `Kobako::ServiceError` are stable public contracts

#### Control — what kobako controls / depends on

**Controls:**
- The entire guest execution environment: mruby interpreter lifecycle, Wasm memory, and capability state
- Handle lifecycle — the guest holds only an opaque integer ID; the Host Gem owns the mapping from ID to host object and all allocation/deallocation decisions
- The host↔guest message codec: MessagePack encoding with two registered ext types (Capability Handle `0x01`, Exception envelope `0x02`)
- Error attribution: the decision logic that maps execution outcomes to the three error classes

**Depends on:**
- `wasmtime` crate (via the private native extension) as the Wasm execution engine
- A pinned mruby release tarball as the guest language runtime embedded in `kobako.wasm`
- `wasi-sdk` toolchain to produce the `wasm32-wasip1` binary at build time
- Host-side and guest-side codec implementations maintained independently; round-trip fuzz tests are the consistency guarantee
- Host App to define and inject Service objects; kobako does not constrain Service shape or method signatures

---

### Feature List

The following features constitute the complete observable surface of the `kobako` gem. Behavior details for each feature are specified in the Behavior layer.

| # | Feature | Role |
|---|---------|------|
| F-01 | Sandbox instantiation | Host Gem |
| F-02 | Service namespace declaration | Host Gem |
| F-03 | Service member binding | Host Gem |
| F-04 | Synchronous script execution | Host Gem + Guest Binary |
| F-05 | Guest-initiated RPC dispatch | Host Gem + Wire Spec |
| F-06 | Capability Handle encoding and referencing | Host Gem + Wire Spec |
| F-07 | Three-class error attribution and raising | Host Gem |
| F-08 | Guest output capture | Host Gem + Guest Binary |
| F-09 | Host–guest message codec | Wire Spec (both sides) |
| F-10 | Reproducible build pipeline | Build tooling |
| F-11 | Multi-layer test and benchmark suite | Quality pipeline |

---

### User Journeys

The following journeys describe the primary ways actors use kobako end-to-end. Each journey is a discrete, runnable scenario that covers one or more Impacts stated in Intent.

---

#### J-01 — LLM agent author runs model-generated code with curated capabilities

**Context**
An LLM agent framework author has a pipeline that feeds model-generated Ruby scripts to kobako at runtime. The Host App holds credentials (API keys, database connections); the generated scripts are untrusted and structurally unknown in advance. The author needs structured results back and must ensure no generated script can exfiltrate credentials or corrupt host state.

**Action**
1. The Host App creates a `Kobako::Sandbox` and declares Service namespaces for the capabilities the generated scripts may legally call (e.g., a key-value lookup, a write-access log sink).
2. For each model-generated script, the Host App calls `Sandbox#run` with the script string, passing no additional configuration at call time.
3. The Host App reads the return value of `#run` as the structured result of the script's final expression.

**Outcome**
The Host App receives a deserialized Ruby value for every successful execution. Generated scripts that exceed their declared capabilities receive a `Kobako::ServiceError` (undefined member), scripts with Ruby errors raise `Kobako::SandboxError`, and Wasm-level failures raise `Kobako::TrapError` — the agent framework routes each class differently (retry, log, restart sandbox). At no point can a generated script read host memory or call methods not bound as Service members.

---

#### J-02 — Host App developer integrates kobako into an existing service

**Context**
A Host App developer is adding kobako to a running Rails or Rack application for the first time. They need to understand the one-time configuration steps and the per-request execution contract before writing any business logic.

**Action**
1. The developer adds kobako to the project's gem dependencies and installs it; the native extension compiles from source.
2. The developer creates a `Kobako::Sandbox`, calls `define` to declare one or more Service namespaces, and calls `bind` on each group to attach host objects as named members.
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
Each submission executes inside an isolated Wasm boundary. A submission that crashes, loops, or attempts to escape receives a `Kobako::TrapError` or `Kobako::SandboxError`; neither outcome affects subsequent submissions. The operator receives the script's result value and captured output for every submission that completes. No submission can read another submission's guest output or access host resources beyond the bound grading Service.

---

#### J-04 — No-code platform evaluates user-defined expressions per request

**Context**
A no-code or low-code platform builder allows end users to write Ruby expressions in formula fields or webhook filter rules. These expressions are evaluated on every incoming event or record. The platform needs sub-second evaluation latency, per-user capability scoping, and the guarantee that a broken user expression does not disrupt the platform's own process.

**Action**
1. The platform creates one `Kobako::Sandbox` per tenant, binding a Service member that exposes the current record or event payload as a read-only object.
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

<!-- Behavior layer: append after Scope -->
<!-- Refinement layer: append after Behavior -->
