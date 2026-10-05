# SPEC.md — kobako

## Intent

Purpose, users, impacts, and non-goals are stated in [`docs/intent.md`](docs/intent.md); every concept is named in [`docs/spec/glossary.md`](docs/spec/glossary.md).

---

## Scope

### System Boundary

#### Responsibility — what kobako does / does not do

**Does:**
- Provide an in-process mruby execution environment isolated by a Wasm boundary
- Bundle a curated guest standard library composed from two sources: allowlisted mruby core-extension mrbgems (Array / Enum / Hash / Numeric / Object / Proc / Range / String / sprintf / Symbol / Error / Metaprog) and Rust capability gems linked into the Guest Binary shell (the worked examples are the IO / Kernel surface, the Regexp / MatchData surface, and the JSON parse / generate surface). The mrbgem half is gated by a strict allowlist — the single source of truth for which mrbgems enter the binary — whose security rationale is documented inline with the build config. Both sources obey the same constraint: pure compute plus the explicitly mediated capabilities, never ambient I/O, network, sleep, or random-seed access.
- Deny the guest every ambient nondeterminism source — wall-clock time, monotonic time, and host entropy — at the Wasm/WASI boundary under the default `hermetic` profile, so guest execution depends only on its source, snippets, and injected Service responses; the `permissive` profile trades this denial away explicitly
- Expose a Ruby API for Host Apps to bind host objects as callable Services at constant-path names
- Execute a mruby source synchronously via `Sandbox#eval` and return its last expression as a deserialized Ruby value
- Register snippets on a Sandbox via `Sandbox#preload` (mruby source via `code:` plus `name:`, or RITE bytecode via `binary:` alone), then dispatch into a named entrypoint constant via `Sandbox#run(target, *args, **kwargs)` and return the entrypoint's `#call` value
- Install an Extension on a Sandbox via `Sandbox#install` — a guest idiom (`source`) plus an optional host `backend` bound at a constant path — composing `#preload` and `#bind` into one setup unit; the guest-visible backend object is either fixed for the Sandbox's life or resolved fresh at every invocation from its provider
- Route guest-initiated Transport requests to the correct host Service object and return the serialized result
- Represent Ruby objects outside the wire type mapping as opaque Capability Handles in both directions across the boundary — objects returned by Service methods (guest→host return path) and objects supplied as `#run` arguments (host→guest argument path); allow the guest to reference those handles in subsequent calls
- Capture guest stdout and stderr into separate in-process buffers and expose them to the Host App
- Classify every execution failure into exactly one of three typed error classes and raise it to the Host App
- Ship the pure default `kobako.wasm` inside the gem alongside a source-only native extension; provide a single build command that produces both artifacts from a clean clone on Linux or macOS. Optional capability variants (`kobako+<cap>.wasm`) are not bundled — they ship as downloadable Release assets a Host App points a Sandbox at; the variant matrix and packaging policy live in [`docs/variants.md`](docs/variants.md)
- Maintain a four-layer test suite and six regression benchmarks that perceive performance drift against a committed anchor baseline across releases

**Does not do:**
- LLM integration, agent frameworks, or prompt engineering — the Host App connects kobako to any LLM
- General-purpose Wasm runtime binding — the native extension is a private implementation detail and exposes no Wasm engine types to the Host App or downstream gems
- mruby upstream development or redistribution — kobako consumes a pinned mruby release tarball unchanged
- Bundle any guest mrbgem that grants access to I/O, networking, sleep, random-seed sources, or syscalls beyond compute and memory — the host capability surface is mediated exclusively through Service injection. I/O, networking, and syscalls rest on the strict allowlist mechanism above; ambient wall-clock time and host entropy are additionally denied at the Wasm/WASI boundary under the default `hermetic` profile, so that guarantee does not rest on the allowlist alone.
- Async or yield-resume execution — all execution is synchronous and blocking; snapshot/resume is not provided
- Multi-tenant billing, SLA management, deployment, or operational tooling
- Windows platform support — Linux and macOS only

#### Interaction — input assumptions / output guarantees

**Input assumptions:**
- The Host App supplies a valid mruby source string to `#eval` at call time, or a valid `target` plus arguments to `#run`
- Service objects provided by the Host App respond to whatever methods guest code will call; kobako does not validate Service shape
- The host machine has Rust/Cargo available to compile the native extension from source at gem install time
- A `Kobako::Sandbox` may be invoked concurrently from multiple Ruby Threads, on distinct Sandboxes or a shared one; a single Thread runs at most one invocation at a time. A host Service object bound once and shared across concurrently-invoking Threads must itself be thread-safe — a `provider:` or `ctx.bind` object is resolved per invocation and carries no such obligation.

**Output guarantees:**
- Every Sandbox invocation (`#eval` or `#run`) either returns a frozen `Kobako::Execution` or raises exactly one of `Kobako::TrapError`, `Kobako::SandboxError`, or `Kobako::ServiceError` — no other outcome is possible. The `Execution` carries that run's `#value` (`#eval`'s last-expression value or `#run`'s entrypoint return value) together with its output captures and usage; a raised error carries the same `Execution` on `#execution`.
- Guest stdout and stderr are always available as separate byte buffers after execution and contain no protocol bytes; truncation, when triggered by a configured cap, is observable via separate predicates on that run's `Execution` and never appears as inline content within the byte streams
- No capability state carries between invocations on the same Sandbox instance — successive or concurrent — regardless of verb; each invocation runs on its own per-invocation state
- The `kobako` gem name, the public Ruby class names below, and the documented public methods on those classes are stable public contracts:

  | Stable public surface | Members |
  |-----------------------|---------|
  | `Kobako::Sandbox` | `#bind` (with no object it declares a fillable), `#install`, `#preload`, `#eval`, `#run` (each accepting an optional `{ \|ctx\| ctx.bind(...) }` per-eval override block); configuration readers `#wasm_path` and `#options` (the `Kobako::SandboxOptions` value object), and the six option readers `#timeout` / `#memory_limit` / `#stdout_limit` / `#stderr_limit` / `#profile` / `#gvl` that forward to it |
  | `Kobako::Pool` | `.new(slots:, checkout_timeout:, **sandbox_keywords)` — the splat forwards every `Kobako::Sandbox.new` keyword verbatim — with a per-Sandbox setup block; checkout verb `#with` |
  | `Kobako::Execution` | The frozen result of one `#eval` / `#run`, returned on success and carried on a raised invocation-outcome error's `#execution`. Readers: `#value` and the `#failed?` predicate (`true` iff the run failed); output readers `#stdout` / `#stderr`; truncation predicates `#stdout_truncated?` / `#stderr_truncated?`; usage reader `#usage`. The reusable Sandbox keeps no observables of its own — every capture and usage reader lives here |
  | Error classes | `Kobako::TrapError`, `Kobako::TimeoutError`, `Kobako::MemoryLimitError`, `Kobako::SandboxError`, `Kobako::BytecodeError`, `Kobako::HandleExhaustedError`, `Kobako::ServiceError`, `Kobako::NoServiceError`, `Kobako::ServiceArgumentError`, `Kobako::BlockError`, `Kobako::YieldValueError`, `Kobako::SetupError`, `Kobako::ModuleNotBuiltError`, `Kobako::PoolTimeoutError`; each invocation-outcome error carries the failed run's `Execution` on `#execution` |
  | `Kobako::Handle` | Named publicly so Host Apps can pattern-match on it inside a `rescue` block; its constructor is internal to the Host Gem — Handles enter Host App code only as fields on raised error instances, never via direct construction |
  | `Kobako::Context` | The per-invocation object yielded to the `#eval` / `#run` override block. Exposes `#bind(path, object)` to override a declared path's object for that one invocation. Not retained by the Host App — valid only inside the block, and spent once it returns |
  | `Kobako::Unresolved` | The sentinel backing a fillable Service path — `#bind(path)` with no object, sugar for `#bind(path, Kobako::Unresolved)`. It reserves the path's Frame 1 slot so the guest sees the constant while the host defers the object; a guest dispatch to an unfilled fillable fails closed, surfacing as `Kobako::ServiceError` when left unrescued. A single shared constant a Host App may name explicitly at a `bind` site |
  | `Kobako::Extension` | The Extension contract, provided as a value object — `.new(name:, source:, backend:, depends_on:)` with `backend` and `depends_on` optional — together with the nested `Kobako::Extension::Backend` value object, declared as `.new(path:, object:)` (static), `.new(path:, provider:)` (per-invocation), or `.new(path:)` (fillable, defaulting to `Kobako::Unresolved`); `object:` and `provider:` together raise `ArgumentError`. `Sandbox#install` duck-types on the four readers `name` / `source` / `backend` / `depends_on`, so a Host App or gem may pass any conforming object in place of the bundled value type |

#### Control — what kobako controls / depends on

**Controls:**
- The entire guest execution environment: mruby interpreter lifecycle, Wasm memory, and capability state
- Handle lifecycle — the guest holds only an opaque integer ID; the Host Gem owns the mapping from ID to host object and all allocation/deallocation decisions
- The host↔guest message codec: a fixed-layout core envelope carrying an opaque payload, with MessagePack as the default payload codec and its two registered ext types (Symbol `0x00`, Capability Handle `0x01`)
- Error attribution: the decision logic that maps execution outcomes to the three error classes

**Depends on:**
- A Wasm execution engine (via the private native extension)
- A pinned mruby release tarball as the guest language runtime embedded in `kobako.wasm`
- A WASI-compatible toolchain available on Linux and macOS to build kobako.wasm
- Host-side and guest-side codec implementations maintained independently; round-trip fuzz tests are the consistency guarantee
- Host App to define and inject Service objects; kobako does not constrain Service shape or method signatures

---

### User Journeys

The following journeys describe the primary ways actors use kobako end-to-end. Each journey is a discrete, runnable scenario that covers one or more Impacts stated in Intent.

---

#### J-01 — LLM agent author runs model-generated code with curated capabilities

**Context**
An LLM agent framework author has a pipeline that feeds model-generated Ruby sources to kobako at runtime. The Host App holds credentials (API keys, database connections); the generated sources are untrusted and structurally unknown in advance. The author needs structured results back and must ensure no generated source can exfiltrate credentials or corrupt host state.

**Action**
1. The Host App creates a `Kobako::Sandbox` and binds Services for the capabilities the generated sources may legally call (e.g., a key-value lookup, a write-access log sink).
2. For each model-generated source, the Host App calls `Sandbox#eval` with the source string, passing no additional configuration at call time.
3. The Host App reads the return value of `#eval` as the structured result of the source's final expression.

**Outcome**
The Host App receives a deserialized Ruby value for every successful execution. Generated sources that exceed their declared capabilities receive a `Kobako::ServiceError` (undefined target), sources with Ruby errors raise `Kobako::SandboxError`, and Wasm-level failures raise `Kobako::TrapError` — the agent framework routes each class differently (retry, log, restart sandbox). At no point can a generated source read host memory or call methods not bound as Services.

---

#### J-02 — Host App developer integrates kobako into an existing service

**Context**
A Host App developer is adding kobako to a running Rails or Rack application for the first time. They need to understand the one-time configuration steps and the per-request execution contract before writing any business logic.

**Action**
1. The developer adds kobako to the project's gem dependencies and installs it; the native extension compiles from source.
2. The developer creates a `Kobako::Sandbox` and calls `bind` to attach host objects as Services at constant-path names.
3. At request time, the developer calls `Sandbox#eval` with a source string and uses the returned `Execution`'s `#value` as the execution result; they also read its `#stdout` and `#stderr` for any guest log output.
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
3. The operator calls `Sandbox#eval` with the student's source string and collects the returned `Execution`'s `#value` and its `#stdout` / `#stderr` output.
4. The operator repeats this for each submission without restarting the host process.

**Outcome**
Each submission executes inside an isolated Wasm boundary. A submission that crashes or attempts to escape receives a `Kobako::TrapError` (or one of its subclasses) or `Kobako::SandboxError`; neither outcome affects subsequent submissions. Each Sandbox enforces a configurable per-invocation wall-clock timeout (default 60 s) and linear-memory delta cap (default 1 MiB) that bounds how far guest `memory.grow` may push past the linear-memory size observed when the invocation entered; submissions exceeding either raise `Kobako::TimeoutError` or `Kobako::MemoryLimitError` respectively, and never block the calling thread beyond the configured timeout. The Host App owns higher-level policy (queue-level fairness, per-student daily caps, retry semantics) above these per-invocation caps. The operator receives the result value and captured output for every submission that completes. No submission can read another submission's guest output or access host resources beyond the bound grading Service.

---

#### J-04 — No-code platform evaluates user-defined expressions per request

**Context**
A no-code or low-code platform builder allows end users to write Ruby expressions in formula fields or webhook filter rules. These expressions are evaluated on every incoming event or record. The platform needs sub-second evaluation latency, per-user capability scoping, and the guarantee that a broken user expression does not disrupt the platform's own process.

**Action**
1. The platform creates one `Kobako::Sandbox` per tenant, binding a Service that exposes the current record or event payload as a read-only object.
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
4. For `ServiceError`, the developer treats it as a capability-level fault (a Service call failed) and applies the same retry or alerting policy as any other service failure in the Host App. Where retrying is the policy, the developer narrows to the `Kobako::NoServiceError` and `Kobako::ServiceArgumentError` branches first — those are the calls that never reached a Service method, so a retry cannot change the outcome.

**Outcome**
The developer can route each failure class through the Host App's existing error-handling infrastructure without inspecting error messages. The three-class taxonomy gives the developer a reliable signal for triage: infrastructure fault (TrapError), authored-code fault (SandboxError), or downstream-service fault (ServiceError); the named `ServiceError` subclasses narrow that last one further without widening what a caller writes, since one `rescue Kobako::ServiceError` still catches every Service failure. This attribution is guaranteed by kobako regardless of whether the failure originated in an `#eval` source or a `#run` entrypoint.

---

#### J-06 — Host App exposes a block-yielding Service

**Context**
A Host App developer is building a Service that iterates over a collection on the host side and wants each element to be processed by a guest-supplied block (similar to `Array#each` semantics). The Service's natural Ruby form takes a block; the developer wants guest code to call it as `MyEach.run(items) { |x| ... }` without learning a different API for the sandboxed environment.

**Action**
1. The developer defines a Ruby class whose method accepts a block (`def run(items, &blk); items.each { |x| yield x }; end`) and binds an instance as a Service at a constant-path name.
2. The guest writes `Service::MyEach.run([1, 2, 3]) { |x| x * 2 }` — the block is part of the guest code, not part of the host code.
3. The Host App calls `Sandbox#eval` (or `#run` into an entrypoint that contains the call site) and reads the return value.

**Outcome**
The Service method's `yield x` invokes the guest block once per iteration, returning each block result back to the host method as the value of `yield`. The Service method observes its block as an ordinary Ruby Proc with loose arity; the guest-side block executes inside the Wasm sandbox and remains isolated from host state. A `break` inside the guest block terminates the Service method early with the break value, matching standard Ruby semantics. A `next` (or natural fall-through) returns the block value to `yield` and execution continues. Exceptions raised inside the block propagate to the `yield` point where the Service method may rescue or let them flow up. The developer writes the Service in idiomatic Ruby; the sandbox boundary is invisible from the Service method's perspective.

---

#### J-07 — Host App preloads a worker and dispatches many invocations

**Context**
A Host App developer is building a request handler whose business logic is supplied as a Ruby source loaded from disk or a config store. The source defines a stable "worker" entrypoint that processes one request per invocation. The developer wants to pay parsing cost once at setup time, then dispatch many requests through the same Sandbox without re-sending the source on every call.

**Action**
1. The developer creates a `Kobako::Sandbox`, calls `bind` to expose Services, then calls `sandbox.preload(code: source, name: :Worker)` once at startup. The `:Worker` source defines a top-level constant `Worker` that responds to `#call(request, opts = {})`.
2. At request time, the developer calls `sandbox.run(:Worker, request, **opts)` for each incoming request.
3. The developer reads the returned `Execution`'s `#value` as the worker's response and its `#stdout` / `#stderr` for any guest log output.

**Outcome**
The `Worker` snippet replays into every invocation's canonical boot state, so per-invocation isolation holds — no state from request N leaks to request N+1. The host normalizes the `:Worker` Symbol, resolves it on top-level `Object`, and dispatches into `Worker.call(request, opts)`; the return value flows back as an ordinary Ruby value. Backtraces produced inside `Worker.call` are attributed to `(snippet:Worker):line`, giving the developer a clear locator. Errors follow the same three-class taxonomy as J-05; `Kobako::UndefinedEntrypointError` — a `SandboxError` — surfaces when the developer dispatches a name the preload table does not provide, carrying the `#name` that was asked for and the `#available` names it could have been, allowing immediate diagnosis without inspecting the guest source.

---

#### J-08 — Host App serves concurrent requests from a warm Sandbox pool

**Context**
A Host App developer runs the J-07 worker pattern inside a multi-threaded web server. Each request needs an exclusively-held, already-set-up Sandbox, and the request rate makes per-request `Sandbox.new` plus `bind` / `preload` setup an unacceptable cost. The number of concurrently live Sandboxes must stay bounded.

**Action**
1. At boot, the developer creates `Kobako::Pool.new(slots: 5) { |sandbox| ... }`, performing all `bind` / `preload` setup inside the block.
2. Each request handler wraps its work in `pool.with { |sandbox| sandbox.run(:Worker, request) }`.
3. The handler reads the returned `Execution`'s `#value`, `#stdout` / `#stderr`, and `#usage` exactly as it would from a directly constructed Sandbox.

**Outcome**
Each request holds one pooled Sandbox exclusively for the duration of its block; concurrent requests beyond `slots` wait for a checkin, and a wait past `checkout_timeout` raises `Kobako::PoolTimeoutError` so the handler can shed load explicitly. Setup cost is paid once per pooled Sandbox, not once per request; per-invocation isolation plus the per-run `Execution` each invocation returns ensure no request observes another request's state or output. A request whose invocation raised `Kobako::TrapError` simply lets the error propagate — the pool discards that Sandbox at checkin and refills the slot on demand, so the next checkout never receives an unrecoverable Sandbox.

---

#### J-09 — Host App installs an Extension so guest code uses a native idiom

**Context**
A Host App developer wants guest scripts to call a familiar constant — a `File` that reads and writes through a host-controlled store — instead of a bespoke bound-Service name. Path arithmetic (`File.join`) should run in-guest with no round-trip, while reads and writes must cross to a host backend that enforces access policy. The developer holds a backend object (or authors one) that answers the read/write calls; the guest idiom itself is a small mruby source.

**Action**
1. The developer obtains an Extension — an object exposing `name`, `source` (the mruby idiom), an optional `backend` (a `Kobako::Extension::Backend` pairing a constant path with a static `object:`, a per-invocation `provider:`, or neither for a fillable), and `depends_on` — authored in-house or supplied by a gem.
2. Before the first invocation, the developer calls `sandbox.install(extension)` (optionally several, or a splatted array). To keep per-invocation state from leaking, the developer declares a `provider:` so a fresh backend object is resolved each invocation; a shared, read-only backend is declared as a static `object:` instead, or the backend is left fillable and supplied per invocation through a `{ |ctx| ctx.bind(...) }` override.
3. At invocation time, the developer calls `Sandbox#eval` (or `#run`) with a script that calls the installed constant — `File.join(a, b)` locally, `File.read(path)` and `File.write(path, data)` across to the backend.

**Outcome**
The guest resolves `File.join` in-guest with no round-trip and dispatches `File.read` / `File.write` to the bound backend under the same isolation and reflection guarantees as any bound Service. A `provider:` yields a fresh backend per invocation, so a write in one invocation is invisible to the next; a static `object:` shares one object across invocations; a fillable backend answers only for an invocation whose `ctx.bind` override supplied it, and otherwise fails closed as an undefined-target `ServiceError`. Installing an Extension whose `depends_on` names an Extension the developer did not install raises `ArgumentError` at the first invocation before the guest runs, naming the missing dependency. When no backend is bound for the idiom, the guest's pure methods still run and its privileged methods fail as an undefined-target `ServiceError`, matching J-01's capability-scoping outcome.

---

## Refinement

### Wire

The host↔guest wire is specified in [`docs/wire-contract.md`](docs/wire-contract.md) (the abstract shape) and [`docs/wire-codec.md`](docs/wire-codec.md) (the two encoding layers, the ABI surface, and the consistency guarantee).

---

### Implementation Standards

#### Architecture

The kobako codebase is split into top-level source areas with a strict boundary between them:

- **`lib/`** — the Host Gem Ruby surface. Contains `kobako.rb` (the main entry point that loads the native extension and defines the public API) and `lib/kobako/` sub-modules (error class definitions, codec helpers, Transport value objects and Dispatcher, Catalog registries — `Services` / `Snippets` / `Handles`). This is the only layer the Host App interacts with directly.
- **`ext/kobako/`** — the private native extension (the `kobako` Rust crate). A thin magnus shim over the host crates in `crates/`: it registers the Ruby classes, bridges the dispatch Proc, and maps the neutral error channels onto the `Kobako::*` classes. This is a private implementation detail of the Host Gem; it is never intended as a reusable wasmtime binding and exposes no Wasm engine types to the Host App or downstream gems.
- **`crates/`** — the publishable Rust crates a non-Ruby embedder builds on: `kobako-transport` (the fixed tier — the core envelope and the ABI surface, shared by both sides of the wasm boundary and depending on nothing), `kobako-codec` (the payload codecs, one namespace per schema), `kobako-runtime` (the engine-neutral runtime contract), and `kobako-wasmtime` (the wasmtime driver — owns the Wasm engine lifecycle and implements the host-side import function `__kobako_dispatch`). The ext consumes `kobako-transport`, `kobako-runtime`, and `kobako-wasmtime` as path dependencies; the guest crates consume `kobako-transport` the same way across the workspace boundary, and `kobako-codec` only when they speak its schema.
- **`wasm/`** — the Guest Binary source (`kobako-wasm` Rust crate, target `wasm32-wasip1`). This is build-time only; it is compiled to `data/kobako.wasm` and excluded from the published gem alongside build tools (`vendor/`, `tasks/`, `build_config/`).
- **`data/kobako.wasm`** — the pre-built Guest Binary artifact. Produced at release time on the publisher's machine and shipped inside the gem. End users receive this file at install time; they never need to recompile the Wasm side.

The boundary rule is: **`ext/` is private to the Host Gem and must never be imported by downstream gems**; `lib/` is the stable public surface. Three Cargo workspaces keep the dependency graphs separate: the root workspace (`ext/kobako` only), `crates/` (the publishable crates), and `wasm/` (the guest). The root `Cargo.toml` excludes `wasm/`, `vendor/`, and `crates/` — the ext and the guest workspace consume `crates/` members as plain path dependencies (`kobako-transport` and `kobako-codec` are engine-free, so their presence in the guest graph pulls in no host-only code), and the isolation prevents host-only crates (e.g., `wasmtime`) from appearing in the wasm32 dependency graph.

#### Design Patterns

The following patterns are enforced project-wide and apply at every layer:

- **Wire evolves only by ABI version bump** — see [`docs/wire-codec.md`](docs/wire-codec.md) § ABI Version for the governing statement. Design implication: never add a wire field gated on a flag or negotiated at runtime; a wire change is a new ABI version, and the Wire Spec at any one version is a single coupled artifact.
- **Round-trip fuzz is the consistency guarantee.** The host-side codec is implemented in pure Ruby under `lib/kobako/codec/` and is loadable at `require` time before the native extension is available; the Rust-side codec is implemented under `crates/kobako-codec/src/msgpack/` and shared by the Guest Binary and any Rust embedder. The two implementations share no source code — the deployment model (the gem must `require` cleanly without a built native extension, and `wasm32-wasip1` cannot embed Ruby) forbids a single codec. Correctness is established by bidirectional round-trip fuzz covering all 11 wire types and both ext types.
- **Codec depends on value objects.** The Codec layer registers `Kobako::Handle` as its ext 0x01 decode target. It is a top-level value object (not nested under `Transport`) precisely so this holds: the dependency direction is Codec → value object; neither the value object nor the Transport layer depends on Codec. This makes it loadable without the codec available and keeps the codec a pure transformation over a known set of host-side types.
- **Three-layer error attribution is two-step** — a trap first, then the Outcome envelope's tag, as [`outcome.md`](docs/spec/behavior/outcome.md) states. Design implication: error classification is a pure function of `(trap?, outcome_tag)`; exit codes, stdout, and stderr are never inputs to that function at either step.
- **Built ahead for a published platform, from source for the rest.** The gem carries a native extension per published Ruby platform, cross-compiled at release time, so installing on one of them reaches no Rust toolchain; any other platform compiles `ext/kobako/` from source instead. `data/kobako.wasm` is platform-agnostic and ships built on every path, so no install needs a WASI toolchain. Which platforms are published is `.github/workflows/release.yml`'s to say, and the gem encodes that set nowhere else.
- **Build-time vendor isolation.** `vendor/wasi-sdk/` and `vendor/mruby/` are fetched from official release tarballs at build time and are never committed to the repository. The versions are pinned by the `beni` gem that fetches them, so one declaration governs every consumer of that toolchain rather than each repository keeping its own. This avoids git submodule pointer maintenance and guarantees cross-environment reproducibility.
- **Fix the bottom layer, not the top.** When a gap is found in a low-level interface (codec type coverage, setjmp/longjmp flag, Wire Spec field, `Catalog::Handles` guard, Panic envelope schema), the fix is applied to the interface layer itself. Working around a low-level gap in a higher-level capability or application layer is not permitted.
- **Process-scope Engine and Module cache.** The wasmtime Engine and the compiled Module for `data/kobako.wasm` are cached at process scope by the `kobako-wasmtime` driver crate. The first `Kobako::Sandbox` constructed in a process pays Engine init and Module compile; every subsequent Sandbox in the same process — regardless of which Thread constructs it — amortizes against this shared state. The cache is implicit; the Host App has no API to inspect, warm, or invalidate it. This pattern is what makes the Sandbox-per-tenant, Sandbox-per-Thread, and shared-Sandbox shapes practical.

##### Invariants

The following invariants hold across every layer of the system. Each is a hard rule; no layer may violate them.

| Invariant | Applies to | Enforcement |
|-----------|-----------|-------------|
| A bound host object is a `Service`, addressed at a constant path (not "tool" or generic names); the guest sees it as a constant that `extend`s `Kobako::Proxy`, and a path prefix is a grouping that materializes as a guest module | All layers | Documentation |
| Wire `target` for a dispatch Call is either a bound constant's path (`"MyService::KV"`, or a single segment `"File"`) or a Capability Handle id; the two forms are discriminated by the envelope's explicit `kind` tag, never by the shape of `target` itself | Core envelope, every peer | Test-time |
| Error attribution is determined solely by `(trap?, outcome_tag)` — stdout, stderr, and exit codes are excluded from attribution logic | Host Gem, error handling | Test-time |
| stdout and stderr carry only user-observable guest output; no kobako protocol bytes appear on these channels | Guest Binary, Host Gem | Test-time |
| `#stdout` and `#stderr` byte content never includes truncation sentinels; truncation status is observable only via `#stdout_truncated?` / `#stderr_truncated?` | Host Gem | Test-time |
| An invocation (`#eval` or `#run`) exceeding the configured `timeout` raises `Kobako::TimeoutError` via the trap-attribution path; no other outcome is possible for wall-clock cap exhaustion | Host Gem | Runtime |
| Guest `memory.grow` whose per-invocation delta past the entry-time linear-memory baseline exceeds the configured `memory_limit` traps unconditionally and raises `Kobako::MemoryLimitError`; the host never observes a silent `memory.grow` failure from cap exhaustion | Host Gem | Runtime |
| `Execution#usage` reports its run's `wall_time` and `memory_peak`, sharing its accounting boundary with the matching caps and populated regardless of outcome; `memory_peak` never exceeds `memory_limit` on `MemoryLimitError` | Host Gem | Runtime |
| `Sandbox#eval`'s `Execution#value` is the last mruby expression and `Sandbox#run`'s is the entrypoint's `#call` value, both carried via the Outcome's ok arm. A value the guest hands across the boundary that has no wire representation is rejected rather than coerced — no path substitutes an implicit `inspect` / `to_h` / `to_s` string: an invocation return takes the Panic arm instead, a yield-block result fails the yield round-trip, and a dispatch argument or kwargs value fails at the dispatch call site | Guest Binary, Wire Spec | Test-time |
| `vendor/` is never committed to the repository; build tools fetch release tarballs at build time | Repository, task scripts | Build-time |
| mruby exception unwind is implemented via wasi-sdk setjmp/longjmp (three mandatory compiler flags); direct modification of mruby setjmp call sites is not permitted | Guest Binary build | Build-time |
| Guest Binary target is `wasm32-wasip1`; wasi-preview2 and component model are out of scope | Guest Binary build, Host Gem | Build-time |
| Under the default `hermetic` profile the guest's WASI ambient sources are denied: `wasi:clocks` (wall and monotonic) is frozen and `wasi:random` is a constant stream, so no real time or host entropy reaches guest code; under `permissive` the same sources read the host's live clocks and entropy. The per-invocation `timeout` is measured on the host clock and enforced by the engine on both rungs, independent of the guest's `wasi:clocks` | Runtime | Runtime |
| `Catalog::Handles` IDs are bounded by `0x7fff_ffff` (2³¹ − 1); exceeding the cap raises `Kobako::HandleExhaustedError` immediately — no silent wraparound or truncation | Host Gem, wire layer | Runtime |
| `ext/kobako/` is a private binding for the kobako gem only; no downstream gem may depend on it directly | Architecture | Documentation |
| Handle lifecycle is per-invocation: every invocation (`#eval` or `#run`) mints its own `Catalog::Handles` with the counter starting at 1; Handles from invocation N are invalid in invocation N+1 | Host Gem, Wire Spec | Test-time |
| Handles are never individually released by the guest; the host implementation does not use `ObjectSpace.define_finalizer` for `Catalog::Handles` entries | Host Gem | Documentation |
| Single-Invocation Slot: Runtime holds at most one active Invocation per OS thread for the duration of any invocation (`#eval` or `#run`). Nested host→guest dispatch (Service calls Service via a yielded block which calls another Service) shares the same Invocation; nested dispatch frames stack within it. There is no stack of Invocations — at most one per thread. The slot licenses the guest-side per-thread statics (`MRB` slot, `BLOCK_STACK`, `OUTCOME_BUFFER`), the host-side per-invocation state (active caller pointer, deadline, wall-clock entry, memory peak, captures), and the per-thread GVL nesting state that governs release | Runtime, Host Gem | Runtime |
| The `gvl:` mode is per-Sandbox and fixed at construction: under `:release` an invocation releases Ruby's GVL only for the span of guest execution and re-acquires it for each guest→host dispatch callback; `:hold`, the default, holds the GVL throughout. Releasing changes scheduling only — every isolation, Handle-lifecycle ([`runtime.md`](docs/spec/behavior/runtime.md)), capture, and outcome behavior holds identically, whether concurrent Threads use distinct Sandboxes or share one | Runtime, Host Gem | Runtime |
| The wasmtime driver crate (`kobako-wasmtime`) declares no `magnus` dependency — only `ext/kobako` links `magnus`. The GVL-released span calls into this driver, so the absent declaration is what keeps a Ruby VALUE out of the span's reach by construction; a build gate rejects the declaration in the driver's manifest | Architecture, Host Gem | Build-time |
| `Kobako::Sandbox#preload` accepts `code:` plus `name:` (matching `/\A[A-Z]\w*\z/`) for source, or `binary:` alone for RITE bytecode treated as opaque bytes whose canonical name, when present, lives in the bytecode's `debug_info`; structural failures surface as `Kobako::BytecodeError` during the first invocation's replay; the snippet table is sealed by the first invocation simultaneously with Service registration | Host Gem | Runtime |
| `Kobako::Sandbox#run(target, ...)` resolves `target` (Symbol or String, normalized to Symbol) only as a top-level `Object` constant; `::`-segmented names and any other multi-segment form fail the constant pattern at host pre-flight | Host Gem | Runtime |
| Wire ABI is a closed enumerated set: exactly one host import (`__kobako_dispatch`) and exactly six guest exports (`__kobako_eval`, `__kobako_run`, `__kobako_alloc`, `__kobako_take_outcome`, `__kobako_yield_to_block`, `__kobako_abi_version`); each entry's name and Wasm signature is fixed across an ABI version. Adding a new import or export requires a new SPEC anchor that lifts the enumeration — the closed-set rule itself is unchanged. | Wire Spec, both codec implementations | Build-time |
| Yield round-trip nests strictly within the dispatch frame whose Service method initiated it; nested dispatch frames each receive at most one Yielder and the Yielders stack in LIFO order — they are not interchangeable across frames | Wire Spec, Host Gem | Runtime |
| Guest mruby's `MRB_STR_LENGTH_MAX` is 1 MiB — a guest-side String at or above this size raises `ArgumentError` inside the guest. This is independent of the 16 MiB single-dispatch wire payload limit; a wire payload can approach the 16 MiB cap via composite values (Array, binary), but a single guest String value cannot. | Guest Binary build (mruby config) | Runtime |

#### Testing Style

The test suite is organized into four layers. All four layers must exist and must pass before a release is approved. No single layer may substitute for another.

| Layer | Name | Scope | When it must pass |
|-------|------|-------|------------------|
| 1 | **Codec round-trip fuzz** | Bidirectional wire codec agreement between Host Gem and Guest Binary codec implementations; covers all 11 wire types, both ext types, and nested compositions | Always — any failure is a wire regression that blocks release unconditionally |
| 2 | **Wire integration** | Full Call / Reply exchange through a live Sandbox, including all envelope type variants | Before release |
| 3 | **Ext unit** | `ext/kobako/` internal Rust unit tests and `lib/kobako/` Ruby specs without starting a Sandbox; includes `Catalog::Handles` allocation / release / fetch, `HandleExhaustedError` guard at `0x7fff_ffff`, wire encode/decode boundary values, and wasmtime API wrapper correctness | Before release; the `Catalog::Handles` exhaustion guard is also a required build-pipeline guard (see below) |
| 4 | **End-to-end** | Full Host App → Sandbox invocation (`#eval` for one-shot source; `#run` for entrypoint dispatch into a `#preload`-registered constant) → Service call → result return path; required coverage is enumerated below the table | Before release |

**Layer 4 required coverage:**

- All three error attribution paths (`TrapError`, `SandboxError`, `ServiceError`) with each trigger
- kwargs dispatch — empty kwargs, symbol-key wire form, and Symbol round-trip through args / return values
- Handle chaining — a Service returns a stateful object and the guest uses the Handle as a subsequent dispatch target
- Guest→host Handle restoration — the guest returns a Handle (bare and nested in an Array / Hash) as the `#eval` / `#run` result or as a yield-block result, and the host restores the original host object
- Guest→host Handle arguments — the guest passes a Handle (bare and nested in an Array / Hash, positional or keyword) into a Service dispatch, and the host resolves each to its original host object before the call
- Handle lifecycle over Sandbox teardown, and cross-invocation Handle invalidity — a Handle obtained in invocation N used as a target in invocation N+1 surfaces as `Kobako::ServiceError` with `type="undefined"` when not rescued within the guest
- gvl: scheduling parity and concurrent per-invocation isolation — a `gvl: :release` invocation produces the same outcome as `:hold` for the same program, and Threads invoking concurrently — each on its own Sandbox, or sharing one and distinguishing invocations by their per-invocation `ctx.bind` identity — each mint and round-trip only their own Capability Handles with no cross-invocation misdelivery, over both `#eval` and `#run`. A differential fuzz drives randomized Handle-minting dispatch trees under both modes and across parallel Threads, following the seed-and-coverage harness discipline of the Layer 1 codec fuzz
- Block / yield round-trip — a Service method receives a block via `&block` and yields one or more times; covers each Yield Reply tag (`0x01` ok, `0x02` break with unwind semantics, `0x04` error from a block exception, and the unsupported-`return` path raising at the yield site), lambda-block `break` silent return, and nested dispatch frames
- stdout / stderr isolation from the Transport channel
- Wire-violation edge cases — `len=0`, unknown tag, an ok Outcome with an unrepresentable value

The recommended execution order is Layer 3 → Layer 1 → Layer 2 → Layer 4 (cheapest first; fail fast before starting the Sandbox).

**Layer 1 harness contract** — the Codec round-trip fuzz harness must satisfy two cross-implementer requirements regardless of transport mechanism (in-process FFI, subprocess IPC, or wasmtime-embedded invocation):

- The random seed for each run is sourced from an environment variable, and any failing iteration's failure output includes the seed in use; a failing run is reproducible from the seed alone.
- The generator records which wire types and ext types it exercises; at the end of each run, the harness asserts that all 11 wire types and both ext types were observed at least once. A coverage gap fails the harness independently of any byte-equality failure.

Iteration count and the transport between the two codec implementations are implementer-chosen.

**Build-pipeline guards** — the following checks must run as part of the build step, before the full test suite:

- `Catalog::Handles` ID cap guard: after `ext/kobako/` is compiled, immediately verify that ID `0x7fff_ffff` is successfully allocated and that the next attempt raises `Kobako::HandleExhaustedError`.
- Gemspec files whitelist check: after `gem build kobako.gemspec`, verify that the resulting archive does not contain `vendor/`, `wasm/`, `tasks/`, or `build_config/` content.

**Regression benchmarks** — the following nine benchmarks must be maintained in `benchmark/` with baseline results stored in git. The gate perceives performance drift relative to a committed anchor baseline; it is not a portable performance guarantee, and the anchor's absolute numbers are meaningful only on hardware comparable to the machine that produced them. Each run is gated against a single committed anchor baseline, not the immediately preceding run: a gated case regresses when its cumulative slowdown past the anchor exceeds +10% and clears the measured noise band. That band bounds the uncertainty of the figure being compared: a figure reduced from many samples is uncertain by less than the spread of one of them. The anchor advances only by a deliberate re-bless that records the accepted shift and its justification in writing; until then every run is measured from the same fixed point, so sub-threshold drift accumulates against the anchor instead of resetting each release. A gated case present in a run but absent from the anchor fails the gate rather than passing silently — the anchor must carry every gated case before release proceeds.

| # | Benchmark | What it detects |
|---|-----------|----------------|
| 1 | Cold start latency (`Kobako::Sandbox.new` → first invocation, `#eval` or `#run`) | wasmtime Module load / Engine initialization regression |
| 2 | Transport round-trip latency (single minimal Service call) | Wire codec, import function dispatch, `Catalog::Handles` lookup combined |
| 3 | Codec throughput at varying payload sizes and nesting depths (host and guest sides measured separately) | Unnecessary allocations or codec path regressions |
| 4 | mruby script evaluation time (fixed script, no Transport calls) | Impact of `build_config/wasi.rb` flag changes on VM execution speed |
| 5 | Handle allocation and release throughput (bulk Service return value wrapping) | `Catalog::Handles` internal dictionary and counter performance |
| 6 | Yield round-trip latency (single host-initiated yield into a guest block) | Yield Reply codec, host→guest yield re-entry dispatch (`__kobako_yield_to_block`), and guest-side `BLOCK_STACK` push/pop combined |
| 9 | Entrypoint dispatch latency for the setup-once / dispatch-many path (`#run` into a preloaded constant, across preloaded snippet counts) | Growth on the `#run` entry path, and in the snippet replay every invocation pays for every preloaded snippet |
| 10 | Host glue of one guest→host dispatch, with no Wasm in the measurement window | Growth in the span a Sandbox holds the GVL through even under `gvl: :release` — the term that bounds multi-core speedup |
| 12 | Host-side cost of one invocation, measured against a guest that performs no work | Regressions in invocation setup, envelope handling, and result decoding, which a host-plus-guest total cannot separate |

A benchmark's number is a stable identifier its case labels carry, so the gated set is not a contiguous range: the numbers absent above belong to benchmarks outside the gate.

Benchmark #1 and #4 are the primary indicators of `build_config/wasi.rb` changes. Benchmark #3 must be run across two dimensions independently: (a) fixed payload size, varying nesting depth; (b) fixed depth, varying payload size. Benchmark #6 must isolate the per-yield steady-state cost by amortizing per-dispatch setup over many yields in one dispatch — the J-06 iteration shape, where per-yield cost compounds linearly — in addition to the single-yield latency; it is the primary indicator of regressions on the host-initiated re-entry path that benchmark #2 (guest-initiated dispatch) does not exercise. Benchmark #9 must drive the real Guest Binary and vary the preloaded snippet count, so per-snippet replay reads as a slope rather than a point — replay is paid on every invocation and a Host App's exposure to it scales with a count it chooses; it is the primary indicator of regressions on the `#run` entry path that the `#eval`-driven benchmarks do not exercise. Only its per-invocation figures are gated: snippet registration is paid once per Sandbox and is characterized, not gated. Benchmark #10 must exclude the Wasm boundary and the guest-side codec, so its figure is the host term alone. Benchmark #12's figure is a measured total, never the difference between a host-plus-guest total and a guest budget. Per-run records are stored as `benchmark/results/<date>-<short-sha>.json`. The gate's anchor baseline is one committed file, `benchmark/baseline.json`, that advances only by re-bless and does not track releases automatically — so drift is bounded across releases, not merely within one. Each run additionally records the measurement-method version of every suite it captured. A probe change that makes a suite's figures incomparable with the ones already archived — a case reordered, a measurement window rescoped, a batch introduced — advances that suite's version, and the gate estimates a row's between-run movement from same-version runs alone. A version change is therefore not a regression signal: it is recorded at the re-bless that absorbs it, which is what lets a reader tell the runs a figure may be compared against from the ones that merely precede it.

#### Code Organization

The following directory layout principles govern the repository. The specific test framework, benchmark library, and CI provider are implementation choices and are not pinned here.

**Directory roles (required, not relocatable):**

- `lib/` — Host Gem Ruby surface; public API entry point and sub-modules
- `ext/kobako/` — private native extension; Rust source (`src/`), `Cargo.toml`, `extconf.rb`; compiled to `lib/kobako/kobako.<ext>` by rake-compiler
- `crates/` — every Rust crate that is not wasm-only, so a Rust embedder and the guest crates can each reach it. Only the ext's path-dependency closure (`kobako-transport`, `kobako-runtime`, `kobako-wasmtime`) ships in the published gem; the `crates/` workspace manifest and lock never do
- `wasm/` — Guest Binary Rust source; compiled to `data/kobako.wasm`; excluded from the published gem
- `data/` — pre-built Wasm artifact (`kobako.wasm`); included in the published gem; never manually edited
- `build_config/` — mruby build configuration (`wasi.rb`); build-time only; excluded from the published gem
- `vendor/` — build-time toolchain storage for wasi-sdk and mruby tarballs; not committed; entirely covered by `.gitignore`; excluded from the published gem
- `tasks/` — Rakefile sub-task files, each owning one task group and self-contained enough to be loaded by glob; the Rakefile is the list, so a group is added by adding a file. Excluded from the published gem
- `test/` — every test file, whatever its kind; excluded from the published gem
- `benchmark/` — benchmark scripts and baseline result files; excluded from the published gem
- `docs/` — design documentation; excluded from the published gem

**gemspec files whitelist:** `kobako.gemspec` pins `spec.files` so the published gem contains exactly the install surface: `lib/**/*.rb`, `ext/kobako/**`, the ext's crate path-dependency closure (`crates/kobako-transport/**`, `crates/kobako-runtime/**`, `crates/kobako-wasmtime/**` — never the `crates/` workspace manifest or lock, and a new crate stays out until the ext depends on it), `data/kobako.wasm`, `sig/**` (minus the dev-only `sig/_external/`), `README.md`, `LICENSE`, `CHANGELOG.md`. All other directories (`vendor/`, `wasm/`, `tasks/`, `build_config/`, `docs/`, `benchmark/`, `test/`) are excluded.

**Two build paths, two starting points:**

- *End-user path*: `gem install kobako` → rake-compiler runs `compile_ext` (Rust toolchain required) → `data/kobako.wasm` is already present; wasi-sdk and mruby tarballs are not needed.
- *Developer path*: `git clone` → `bundle install` → `bundle exec rake compile` → the `beni` gem's tasks vendor the pinned wasi-sdk and mruby into `vendor/` and build `libmruby.a`, then this repository's wasm build links `data/kobako.wasm`, then `compile_ext`.

Every build task must be idempotent: the presence of the target file its stage produces short-circuits re-execution, so incremental development only reruns the changed stage. This holds across the boundary — a stage the toolchain gem owns is as re-entrant as one this repository owns.

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
