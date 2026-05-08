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

---

## Behavior

The behaviors below specify observable outcomes for the Sandbox object and its execution contract. Each behavior uses the form **Initial State → Operation → Result / Final State**. Error attribution (TrapError, SandboxError, ServiceError) is covered in a later subsection; where an error branch is noted below, refer to that subsection for full semantics.

---

### B-01 — Construct a new Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | No `Kobako::Sandbox` instance exists. No Guest Binary is running. |
| **Operation** | `Kobako::Sandbox.new` — optionally with `stdout_limit:` and/or `stderr_limit:` keyword arguments (each defaults to 1 MiB). |
| **Result / Final State** | A Sandbox instance is returned. No Guest Binary is started. The stdout and stderr buffers are empty. The Sandbox is ready to accept `#run` calls. |
| **Notes** | `stdout_limit` and `stderr_limit` control the per-run capture ceiling (see B-04). Service declarations and bindings may be made on the Sandbox before or after construction and before the first `#run`. |

---

### B-02 — Invoke `#run(script)` from a fresh Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with zero prior `#run` calls. Zero or more Service members have been bound. The stdout and stderr buffers are empty. |
| **Operation** | `sandbox.run(script_string)` where `script_string` is a valid mruby script. |
| **Result / Final State** | The Guest Binary is booted from cold for this invocation. The script is compiled and executed within the isolated Wasm boundary. `#run` blocks until execution completes. On success, `#run` returns a single deserialized Ruby value — the script's last expression. The stdout and stderr buffers contain any output the script wrote during execution. The Guest Binary instance used for this run is fully retired after the outcome is retrieved. |
| **Notes** | The return value semantics are detailed in B-06. Error outcomes are covered in a later subsection. |

---

### B-03 — Invoke `#run(script)` on a Sandbox that has already run

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance that has completed one or more prior `#run` calls. Service members bound before the first `#run` remain registered. |
| **Operation** | `sandbox.run(script_string)` — any invocation after the first. |
| **Result / Final State** | A fresh Guest Binary instance is booted for this invocation, independent of all prior instances. All capability state (HandleTable contents) from previous runs is fully discarded before the new run begins. Service bindings declared on the Sandbox before the first run remain active and are visible to the new run. `#run` returns the new script's last expression. The stdout and stderr buffers are cleared at the start of this run and contain only output from this invocation. |
| **Notes** | A Handle issued during run N is not reachable during run N+1. This isolation guarantee is unconditional — it holds whether the previous run succeeded or raised an error. Service bindings are never cleared between runs; only capability state is reset. |

---

### B-04 — Read `#stdout` / `#stderr` after `#run` returns

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which `#run` has been called and has returned (either with a value or by raising an error). |
| **Operation** | `sandbox.stdout` or `sandbox.stderr` — either or both, in any order, any number of times. |
| **Result / Final State** | Each reader returns the complete byte content (as a UTF-8 String) that the guest script wrote to its respective output channel during the most recent `#run` invocation. The buffers do not change between successive reads. The content contains no kobako protocol bytes. If the accumulated output exceeded the configured limit, the buffer contains the captured bytes up to that limit followed by a `[truncated]` marker; this truncation does not cause `#run` to raise an error. |
| **Notes** | The buffers remain readable after an error-raising `#run`; the Host App reads them after catching the error. A script may have written diagnostic output before the point of failure. Buffer limits are set at construction time (B-01). |

---

### B-05 — Read `#stdout` / `#stderr` before any `#run` call

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which `#run` has never been called. |
| **Operation** | `sandbox.stdout` or `sandbox.stderr`. |
| **Result / Final State** | Each reader returns an empty String (`""`). No error is raised. |
| **Notes** | This behavior defines the initial buffer state and allows Host Apps to read observation channels unconditionally without guarding against nil. |

---

### B-06 — Return value semantics of `#run`

This behavior refines the Result of B-02 / B-03 by specifying the exact value `#run` produces.

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance, either fresh (per B-02) or post-run (per B-03), with zero or more Service members bound. |
| **Operation** | `sandbox.run(script_string)` — same invocation as B-02 / B-03. |
| **Result / Final State** | When the guest script completes execution without a Wasm trap, `#run` returns the deserialized Ruby value of the script's last mruby expression. If the last expression evaluates to `nil` (including scripts with no explicit return expression), `#run` returns Ruby `nil`. If the last expression produces a mruby object that has no wire representation (i.e. it cannot be encoded through the host↔guest message codec), `#run` raises `Kobako::SandboxError`. All other error outcomes are covered in a later subsection. |
| **Notes** | Exactly one value is returned per `#run` call. There is no mechanism for a script to return multiple values or stream values. The wire-violation path (unrepresentable object) is attributed to the sandbox, not the Wasm engine or a Service call. |

---

### B-07 — Declare a Service Group on a Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which `#run` has not yet been called. No Service Group named `GroupName` exists on this Sandbox. |
| **Operation** | `sandbox.define(:GroupName)` where `:GroupName` is a Symbol matching `/\A[A-Z]\w*\z/` (Ruby constant-name form). |
| **Result / Final State** | A `Kobako::Service::Group` instance is created and associated with this Sandbox under the name `GroupName`. The group has no members yet. The method returns the new `Kobako::Service::Group` instance. The Sandbox registry now contains one additional group entry. |
| **Notes** | `GroupName` must conform to Ruby constant naming (`/\A[A-Z]\w*\z/`); a non-conforming name raises `ArgumentError` (error scenarios covered in Item #7). Declarations are design-time operations: they must be made before `#run` is first called. A group may have zero members at declaration time; members are added via B-08. |

---

### B-08 — Bind a Service Member to a declared Group

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Service::Group` instance (returned by `sandbox.define`) with no member bound under the name `MemberName`. |
| **Operation** | `group.bind(:MemberName, object)` where `:MemberName` matches `/\A[A-Z]\w*\z/` and `object` is any Ruby object (class, instance, or module) that responds to the methods guest code will invoke. |
| **Result / Final State** | `object` is registered as the member named `MemberName` within the group. Guest code can now reach this object via the two-level path `GroupName::MemberName`. The method returns the `Kobako::Service::Group` instance (`self`) to allow chaining. |
| **Notes** | `bind` accepts any Ruby object — class, instance, or module — uniformly; the Host App is responsible for ensuring `object` responds to the methods guest code will call. The bound object's lifecycle is owned by the Host App; the Sandbox registry holds a reference until the Sandbox is garbage-collected. |

---

### B-09 — Declare multiple Service Groups on the same Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with one or more Service Groups already declared. |
| **Operation** | `sandbox.define(:OtherGroup)` with a name distinct from all already-declared groups on this Sandbox. |
| **Result / Final State** | A new, independent `Kobako::Service::Group` is created alongside the existing groups. Each group's members are accessible to guest code only via that group's own namespace (e.g., `GroupA::Member` and `GroupB::Member` are distinct paths with no cross-group visibility). Groups on different Sandbox instances are fully isolated from each other. |
| **Notes** | There is no declared upper limit on the number of groups per Sandbox. Each group name within a Sandbox must be unique (duplicate-declare behavior is specified in B-10). |

---

### B-10 — Re-declare a Service Group that already exists (idempotent define)

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with a Service Group already declared under the name `GroupName`. |
| **Operation** | `sandbox.define(:GroupName)` — same name as an existing group. |
| **Result / Final State** | No new group is created. The existing `Kobako::Service::Group` instance is returned unchanged, with all previously bound members still in place. The Sandbox registry is not modified. |
| **Notes** | Idempotent re-declaration allows Host Apps to retrieve an existing group handle without tracking it externally (e.g., in configuration code spread across multiple files). |

---

### B-11 — Attempt to bind a Member name that is already bound in the same Group

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Service::Group` instance with a member already bound under the name `MemberName`. |
| **Operation** | `group.bind(:MemberName, new_object)` — same member name as an already-bound member. |
| **Result / Final State** | `ArgumentError` is raised. The existing binding is not overwritten. The group's member registry is unchanged. |
| **Notes** | Duplicate binding raises `ArgumentError`; the existing binding is preserved. Error scenarios are covered in full in Item #7. |

---

### B-12 — Guest-initiated RPC call dispatched to a bound Ruby object

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing a mruby script. A Service Member is bound at `GroupName::MemberName`. The guest script holds a reference to the constant `GroupName::MemberName` and calls a method on it. |
| **Operation** | Guest code executes `GroupName::MemberName.method_name(arg1, arg2, key: value)` — a synchronous method call from within the mruby script. |
| **Result / Final State** | The Host Gem resolves the target to the Ruby object bound at `GroupName::MemberName` and invokes `object.public_send(:method_name, arg1, arg2, key: value)`. The Ruby return value is serialized and returned to the guest as the synchronous result of the call — from the guest script's perspective, the call completes as an ordinary synchronous Ruby method invocation. |
| **Notes** | The dispatch is always a single `public_send` — no multi-step traversal occurs within one RPC. Bound Ruby objects receive keyword arguments as Ruby symbols, matching standard Ruby keyword argument conventions. If the target path is not found in the registry, a `ServiceError` is returned to the guest (covered in Item #7). |

---

### B-13 — Service method returns a primitive value

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated RPC call (B-12) has been dispatched. The bound Ruby object's method returns a value that has a wire representation: `nil`, Boolean, Integer, Float, String, binary string, Array, or Hash. |
| **Operation** | The Host Gem's wire codec serializes the return value and delivers it to the guest as the RPC response. |
| **Result / Final State** | The guest script receives the return value as the synchronous result of the method call, deserialized to the corresponding mruby type. The value is indistinguishable from a locally computed mruby value. No entry is added to the HandleTable. |
| **Notes** | The wire codec is the same codec used for `#run` return values (B-06). Values that have no wire representation cause a `Kobako::SandboxError` (see Item #7). Collections (Array, Hash) whose elements are all wire-representable are transmitted in full by value. |

---

### B-14 — Service method returns a stateful object (Host-side Handle allocation)

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated RPC call (B-12) has been dispatched. The bound Ruby object's method returns a Ruby object that is not a primitive wire type — for example, a session object, a connection, or any stateful host resource. |
| **Operation** | The Host Gem's wire layer detects that the return value is not a primitive wire type and automatically registers it in the Sandbox's HandleTable. |
| **Result / Final State** | The host-side object is stored in the HandleTable under a new opaque u32 Handle ID. The guest receives a Capability Handle (an opaque integer token) as the RPC response value, not the object itself. The guest can pass this Handle as the `target` in subsequent RPC calls to invoke methods on the same host-side object. The Host App has no API to create or inspect Handles directly — Handle allocation is an internal wire-layer operation. |
| **Notes** | Handle lifecycle (per-`#run` scope, ABA protection, ID limits) is specified in Item #6. The guest cannot dereference a Handle to a Ruby value; it can only use it as a target in further RPC calls. |

---

### B-15 — Handle ID is allocated with a monotonically increasing counter scoped to a single `#run`

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation has just begun. The HandleTable counter is reset to 1. No entries exist in the table. |
| **Operation** | The Host Gem's wire layer allocates a new Handle for a stateful return value (B-14). |
| **Result / Final State** | The first Handle issued in this run receives ID 1, the second ID 2, and so on. Each ID is unique within the run. The counter never wraps or reuses an ID during a single `#run`. IDs are assigned in allocation order and no ID is skipped unless the exhaustion cap is reached (B-21). |
| **Notes** | ID 0 is reserved as an invalid sentinel; the guest can use `id == 0` as a fast-fail guard. Counter and IDs are reset at the start of every `#run` — IDs from run N are not carried into run N+1 (see B-18). |

---

### B-16 — Guest passes a previously-received Handle as an argument to a Service RPC call

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation is in progress. The guest holds a `Kobako::Handle` (mruby object) obtained from a prior RPC response in the same run. The Handle's internal ID resolves to a live entry in the HandleTable. |
| **Operation** | Guest code invokes a method on a `Kobako::RPC` Service member and passes the `Kobako::Handle` as one of the arguments (e.g., `Store.put(handle, value)`). |
| **Result / Final State** | The Host Gem deserializes the Handle from the wire representation, looks up its ID in the HandleTable, and passes the resolved Ruby object as the corresponding argument to the host Service method. The Service method receives the actual Ruby object, not an ID or token. The method executes and its return value follows the normal primitive (B-13) or stateful (B-14) path. |
| **Notes** | The guest never sees or manipulates the raw integer ID; it holds a `Kobako::Handle` mruby proxy object and calls methods on it or passes it as an argument. If the ID is not found or is marked disconnected, the error path is covered in Item #7. |

---

### B-17 — Chained composition: Handle returned by Service A used as target in a subsequent call to Service B

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation is in progress. Service A has been called via RPC and returned a stateful object; the guest holds `handle_a` (a `Kobako::Handle` proxy). |
| **Operation** | Guest code calls a method directly on `handle_a` (e.g., `handle_a.find(id)`), using the Handle as the RPC target. The wire layer encodes `handle_a` as the `target` field of the Request. |
| **Result / Final State** | The Host Gem resolves `handle_a`'s ID against the HandleTable and invokes `public_send(:find, id)` on the host-side Ruby object that `handle_a` represents. If that call returns another stateful object, a new Handle `handle_b` is allocated and returned to the guest. Each step in the chain is an independent, synchronous RPC; there is no implicit multi-hop traversal within a single wire call. |
| **Notes** | Chain depth is unbounded within a single `#run` as long as each step produces a Handle that survives to the next call. Each intermediate Handle is a first-class entry in the HandleTable and follows the same lifecycle rules as any other Handle. Every host object reachable by the guest must have been explicitly returned by a Service method; there is no implicit intermediate binding path. |

---

### B-18 — Handle issued in run N is presented as a target in run N+1

| Field | Value |
|-------|-------|
| **Initial State** | Run N has completed. The guest (or a script) attempts to retain a Handle ID from run N and presents it as the `target` in a new `#run` invocation (run N+1). At the start of run N+1 the HandleTable has been fully reset: all entries from run N are cleared and the counter restarted. |
| **Operation** | Guest code in run N+1 calls a method using the stale Handle ID as the RPC target. |
| **Result / Final State** | The HandleTable lookup for that ID returns `:undefined` — the ID does not exist in the fresh table. The stale Handle is invalid; the Host Gem treats this as an unrecognized target. The error path (Item #7) is triggered. Run N+1 is not interrupted for other RPC calls that do not reference stale IDs. |
| **Notes** | This outcome is unconditional: even if run N and run N+1 execute the same script with the same Service bindings, no Handle survives the `#run` boundary. The reset happens before the Guest Binary is instantiated for run N+1, so there is no window in which a stale ID could coincidentally match a new entry. |

---

### B-19 — Sandbox is discarded: all Handles for that Sandbox become invalid

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Sandbox` instance exists with zero or more completed `#run` invocations. The HandleTable is owned by this Sandbox instance. |
| **Operation** | The Sandbox instance is garbage-collected or goes out of scope; Ruby reclaims it. |
| **Result / Final State** | The HandleTable and all its entries are destroyed as part of Sandbox teardown. Every Handle that was issued during any `#run` on this Sandbox is permanently invalid. No Handle entry is shared with, transferred to, or reachable from any other Sandbox instance. |
| **Notes** | Handles are not reference-counted and there is no explicit `release` API for individual entries. Validity is scoped to the owning Sandbox and the specific `#run` in which the Handle was issued (B-18). A Handle that was valid in a prior `#run` on this Sandbox is already invalid by the time the Sandbox is collected (per B-18); Sandbox teardown simply removes the ownership root. |

---

### B-20 — Guest cannot construct or dereference a Handle from a raw integer

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation is in progress. The guest mruby script has access to an arbitrary integer value (e.g., `42` or any computed integer). |
| **Operation** | Guest code attempts to use a raw integer as a Handle target for an RPC call — for example, by constructing a `Kobako::Handle`-like object from an integer literal, or by any means other than receiving a Handle from a prior RPC response. |
| **Result / Final State** | No valid `Kobako::Handle` mruby object is produced from a bare integer. The guest mruby API does not expose a constructor that converts an integer to a Handle. A raw integer presented as an RPC target does not carry the Handle wire encoding (`ext 0x01`); the guest-side wire decoder rejects the malformed encoding before the value reaches the HandleTable. The error path is covered in Item #7. |
| **Notes** | The `Kobako::Handle` mruby class holds the u32 ID internally but does not expose it as a readable integer attribute. This prevents guest code from forging capability references. Guest code that received no Handle from a Service call has no legitimate path to construct one. |

---

### B-21 — HandleTable exhaustion: allocation attempt beyond the ID cap

| Field | Value |
|-------|-------|
| **Initial State** | A `#run` invocation is in progress. The HandleTable counter has reached `0x7fff_ffff` (2³¹ − 1), the maximum valid Handle ID. |
| **Operation** | The Host Gem's wire layer attempts to allocate one additional Handle for a new stateful return value. |
| **Result / Final State** | The allocation fails immediately with a `Kobako::SandboxError`. The counter is not incremented, no new entry is written to the HandleTable, and no ID is silently truncated or wrapped. The error is raised to the Host App; the current `#run` terminates. |
| **Notes** | The cap exists because mruby on wasm32 uses `MRB_INT32` (signed 32-bit integers): IDs above `0x7fff_ffff` would arrive at the guest as negative integers, silently corrupting capability references. The fail-fast guard makes the violation visible rather than allowing silent semantic corruption. The error path is covered in detail in Item #7. |

---

### Error Scenarios

Every `Sandbox#run` invocation terminates in exactly one of four outcomes: a return value, `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError`. Attribution is determined by a two-step decision applied after `__kobako_run` returns:

**Step 1 — Trap detection (highest priority).**
If the Wasm engine reports a trap (e.g., wasmtime raises a native trap exception), the outcome is `Kobako::TrapError` regardless of any other state. No outcome bytes are inspected.

**Step 2 — Outcome envelope tag (non-trap outcomes only).**
If no trap occurred, the Host Gem reads the outcome bytes produced by `__kobako_take_outcome` and dispatches on the first-byte tag:

| First-byte tag | Outcome bytes state | Raised class |
|---------------|---------------------|--------------|
| — | Zero-length (`len == 0`) | `Kobako::TrapError` — wire violation fallback |
| `0x01` (result) | Decode succeeds | Return value (no error raised) |
| `0x01` (result) | Decode fails (malformed MessagePack or unrepresentable value) | `Kobako::SandboxError` |
| `0x02` (panic) | Decode succeeds + `origin == "service"` | `Kobako::ServiceError` |
| `0x02` (panic) | Decode succeeds + `origin == "sandbox"` or missing | `Kobako::SandboxError` |
| `0x02` (panic) | Decode fails (malformed envelope) | `Kobako::SandboxError` |
| Any other tag | — | `Kobako::TrapError` — wire violation fallback |

`stdout` and `stderr` bytes do not participate in attribution dispatch. They are always available via `Sandbox#stdout` / `Sandbox#stderr` after a rescue, including after error-raising runs.

---

#### `Kobako::TrapError`

Raised when the Wasm execution engine crashes or when the wire layer detects a structural violation that signals a corrupted guest execution environment. After a `TrapError`, the Sandbox is considered unrecoverable; Host Apps should discard and recreate it before the next execution.

| # | Trigger | Detection point |
|---|---------|-----------------|
| E-01 | Wasm engine trap: OOM, `unreachable` instruction, stack overflow, or import signature mismatch | wasmtime raises a native trap exception; Step 1 fires |
| E-02 | Guest exited without writing any outcome bytes (`len == 0`) | Step 2: zero-length outcome bytes; wire violation fallback |
| E-03 | Outcome first byte is an unknown tag (not `0x01` or `0x02`) | Step 2: unrecognized tag; wire violation fallback |

**Cross-references:** E-02 and E-03 are the wire-violation fallback paths invoked by any malformed Guest Binary output. B-21 (HandleTable exhaustion) raises `Kobako::SandboxError`, not `TrapError`, because the failure is detected before the guest runtime crashes.

---

#### `Kobako::SandboxError`

Raised when the guest execution environment ran to completion but the overall execution failed due to a protocol fault, a mruby runtime error, or a Host Gem–side wire decode failure. The guest Wasm instance is retired normally; the sandbox infrastructure itself is intact.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-04 | Guest mruby script raises an uncaught exception (e.g., `RuntimeError`, `NoMethodError`) that reaches the top level of `__kobako_run` | B-02, B-03 — script execution |
| E-05 | Guest boot script fails to load or compile the user script (`mrb_load_string` error before execution begins) | B-02 — fresh run |
| E-06 | `#run` last-expression result has no wire representation (e.g., a raw mruby `Object` with no MessagePack encoding); outcome tag `0x01` is present but the value field fails to decode | B-06 — return value semantics |
| E-07 | Service method return value is not a primitive wire type and HandleTable allocation fails because the counter has reached `0x7fff_ffff` (2³¹ − 1) | B-21 — HandleTable exhaustion |
| E-08 | Outcome tag is `0x02` (panic) and the panic envelope is malformed or missing required fields | Step 2 attribution; E-04 fallback |
| E-09 | Outcome tag is `0x01` (result) and the result envelope is malformed or fails MessagePack parse | Step 2 attribution; B-06 fallback |
| E-10 | Guest presents an invalid wire payload as an RPC argument (e.g., a raw integer where a Capability Handle ext type `0x01` is required) | B-20 — guest cannot forge Handles |

`sandbox.define(:name)` where `:name` does not match `/\A[A-Z]\w*\z/` raises `ArgumentError` (B-07). `group.bind(:MemberName, obj)` when `MemberName` is already bound raises `ArgumentError` (B-11). Both are Host App programming errors detected at setup time before any guest execution; they do not go through the attribution pipeline and are not classified as `SandboxError`.

---

#### `Kobako::ServiceError`

Raised when the guest execution environment ran to completion, the mruby script itself did not crash, but a Service capability call reported an application-level failure. The error originates in host Service code or in the capability routing layer, not in mruby script logic or the Wasm engine.

`ServiceError` is raised when a panic envelope with `origin == "service"` reaches the host — meaning the mruby script executed a Service RPC that failed and the failure was not rescued within the script.

| # | Trigger | Behavior cross-reference |
|---|---------|--------------------------|
| E-11 | A bound Service method raises a Ruby exception during dispatch; the exception propagates through the RPC response as `status=1`, error `type="runtime"`, and the mruby script does not rescue it | B-12 — RPC dispatch |
| E-12 | The RPC `target` path (e.g., `"GroupName::MemberName"`) does not match any registered Service Member; error `type="undefined"` returned; mruby script does not rescue it | B-07, B-12 — undefined member |
| E-13 | The RPC `target` is a Handle ID that does not exist in the current run's HandleTable (stale Handle from a prior run presented as target in a new run); error `type="undefined"` | B-18 — stale Handle cross-run |
| E-14 | The RPC `target` Handle ID resolves to the `:disconnected` sentinel in the HandleTable; error `type="disconnected"` | B-16 — Handle referencing |
| E-15 | Service method receives arguments that fail the host-side parameter binding (e.g., unknown keyword); error `type="argument"` returned; mruby script does not rescue it | B-12 — RPC dispatch |

A Handle ID from run N presented as an RPC target in run N+1 produces `type="undefined"` because the HandleTable is fully reset at the start of each `#run`; this reaches the host as `Kobako::ServiceError` if the script does not rescue the error response (B-18). A guest attempting to forge a Handle from a bare integer is rejected by the guest-side wire decoder before any RPC reaches the host; that path raises `Kobako::SandboxError` (E-10), not `ServiceError` (B-20).

When a guest script wraps a Service call in `begin/rescue`, the RPC failure is handled within the script; no `ServiceError` reaches the host and `#run` returns normally. `Kobako::ServiceError` is raised to the Host App only when a Service failure is unrescued at the top level of the script.

<!-- Refinement layer: append after Behavior -->

---

## Refinement

### Terminology

This section defines every term used in this specification. Each term has exactly one canonical name; no synonyms are permitted within this document or the kobako codebase public surface.

---

#### Roles

These five roles describe every actor and artifact in the system. All sections of this specification use these names exclusively.

| Term | Definition | Layer |
|------|-----------|-------|
| **Host App** | The Ruby application (Rails / Rack / Sidekiq / CLI) that uses kobako; holds all credentials, policy, and Service objects. Out of scope for design but referenced throughout. | External |
| **Host Gem** | The `kobako` gem itself: the Ruby API layer (`lib/`) plus the private native extension (`ext/`). Exposes the sandbox interface to the Host App, routes RPC calls, and manages Handle lifecycle. | In scope |
| **Guest Binary** | The file `kobako.wasm`, compiled from the `wasm/` Rust crate. Contains the mruby interpreter and RPC client. Is the isolation boundary between host and guest execution environments. | In scope |
| **Service** | A Host Ruby object injected into the sandbox under a two-level name (`Group::Member`). The only mechanism by which guest code can access host resources. | In scope |
| **Wire Spec** | The MessagePack contract governing all host↔guest RPC messages. Not a runtime object — it is a protocol that both Host Gem and Guest Binary implement independently. | In scope |

---

#### Internal Concepts

These are sub-components and runtime concepts owned by the Host Gem. They are not exposed as a public API to the Host App unless explicitly stated.

| Term | Definition | Public? |
|------|-----------|---------|
| **Sandbox** | The runtime unit instantiated by `Kobako::Sandbox`. Owns the Guest Binary lifecycle, Service registry, HandleTable, and output buffers for a single logical execution context. Maps to the Ruby class `Kobako::Sandbox`. | Yes — `Kobako::Sandbox` is stable public API |
| **Registry** | The Host Gem sub-component that maintains Service Group / Member registrations, routes incoming RPC calls to the correct host object, and owns the HandleTable. Not exposed to the Host App. | No |
| **HandleTable** | The host-side mapping from Handle IDs to Ruby objects. Owned by the Registry. Created fresh at the start of each `#run` and fully discarded at the end. Not exposed to the Host App. | No |
| **Handle** | An opaque integer token the guest holds to reference a host-side object returned by a Service call. The guest can pass it as an RPC target or argument in subsequent calls but cannot dereference it to a Ruby value. Maps to the Ruby class `Kobako::Handle` (host side) and the `Kobako::Handle` mruby class (guest side). | Partially — `Kobako::Handle` appears in Host App error handling and output, but the Host App has no API to create or inspect Handles directly |
| **Capability Handle** | A Handle that represents a stateful host-side resource (e.g., a session, connection, or any object that is not a primitive wire type). Transmitted on the wire as MessagePack ext type `0x01`. "Capability Handle" is used when emphasizing the capability-granting semantics; "Handle" is used for brevity elsewhere — both refer to the same concept. | No distinct class — see Handle |
| **Stub** | The mruby VM-internal base class (`Kobako::RPC`) that represents a remote Service Member inside the guest. Guest scripts do not reference Stub directly; they see module constants. All method calls on a Stub are forwarded as RPC calls to the host. Internal to the Guest Binary; not visible to the Host App or guest scripts as a named class. | No |

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
| **HandleTableExhausted** | `Kobako::HandleTableExhausted` | `Kobako::SandboxError` | Handle ID counter reached `0x7fff_ffff` (2³¹ − 1) within a single `#run`; further allocation is impossible |
| **ServiceError::Disconnected** | `Kobako::ServiceError::Disconnected` | `Kobako::ServiceError` | RPC target Handle resolves to the `:disconnected` sentinel in the HandleTable |

**Wire-level error string (not a Ruby class):** The string `"Kobako::WireError"` appears only as the `class` field value in a Panic envelope to signal that the wire layer detected a violation. On the host side this maps to a raised `Kobako::SandboxError`; there is no standalone `Kobako::WireError` Ruby class.

---

#### Service Concepts

These terms describe the two-level Service injection model used to expose host capabilities to guest scripts.

| Term | Definition | Guest-visible form |
|------|-----------|-------------------|
| **Service Group** | A named namespace declared by the Host App via `sandbox.define(:Name)`. Groups are declared at setup time before the first `#run`. The group itself holds no state — it is a container for Service Members. | Ruby module (e.g., `MyService`) |
| **Service Member** | A Host Ruby object bound into a Service Group via `group.bind(:Name, object)`. The Member is the object that receives RPC calls dispatched from guest scripts. | Module constant (e.g., `MyService::KV`) |

---

### Naming Principles

The following principles govern how all names in this specification and in the `kobako` public surface are formed. They are declarative rules, not rationale.

| # | Principle | Applies to |
|---|----------|-----------|
| N-1 | Role names are PascalCase with every word capitalized: `Host App`, `Host Gem`, `Guest Binary`, `Wire Spec` | All role names in this document and in code comments |
| N-2 | All public Ruby classes and modules live under the `Kobako::` namespace | Ruby classes: `Kobako::Sandbox`, `Kobako::TrapError`, `Kobako::SandboxError`, `Kobako::ServiceError`, `Kobako::Handle`, `Kobako::Service::Group` |
| N-3 | The gem name is always lowercase: `kobako` | Gemspec, `require` statements, Bundler references |
| N-4 | The Wasm artifact name is fixed: `kobako.wasm` | Build output, gem packaging, documentation |
| N-5 | Internal Rust crates are named with a hyphen prefix matching the gem: `kobako-wasm` (Guest Binary crate), `kobako-ext` (native extension crate) | `Cargo.toml` package names; not exposed to Ruby |
| N-6 | A concept has exactly one name; no synonyms appear in the same document or public surface | All layers of this specification |
| N-7 | Error class names encode the layer they represent: `TrapError` → Wasm engine layer, `SandboxError` → sandbox/wire layer, `ServiceError` → service/capability layer | `Kobako::TrapError`, `Kobako::SandboxError`, `Kobako::ServiceError` |
