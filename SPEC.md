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
