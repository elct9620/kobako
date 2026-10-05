# SPEC.md — kobako

## Intent

Purpose, users, impacts, and non-goals are stated in [`docs/intent.md`](docs/intent.md); every concept is named in [`docs/spec/glossary.md`](docs/spec/glossary.md).

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
