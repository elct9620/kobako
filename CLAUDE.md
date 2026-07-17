# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

kobako is a Ruby gem that provides an in-process Wasm sandbox for running untrusted mruby scripts. The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing the mruby interpreter; host and guest communicate over a MessagePack-based Transport wire defined in `SPEC.md`.

## Principles

Apply these in order — earlier principles override later ones on conflict.

1. **SPEC.md is the source of truth.** Behavior contracts live in `SPEC.md` or in the `docs/<topic>.md` it indexes; the per-anchor behavior detail lives in `docs/behavior/<aspect>.md`, mapped from each anchor range by the grouping table in `SPEC.md` § Behavior. Behavior anchors form append-only families that are never renumbered; the `rake gate:anchors` gate enforces uniqueness, contiguity, and resolvability across the spec corpus — consult the grouping table for the current families and ceilings. **Cite anchors only where traceability belongs: in tests (the test that verifies an anchor names it) and in prose docs (linking directly to the behavior file that holds it). Implementation and public-API comments state intent — what + why — not anchor citations.** When SPEC is silent, extend it (or the relevant `docs/<topic>.md`) first.

2. **One thing per file; keep files small.** Split a growing module into a façade plus per-responsibility files in a sibling directory — `Kobako::Transport` and `Kobako::Snippet` are the worked examples. In guest capability gems, a file extending an existing core class takes the `_ext` suffix (`string_ext.rs`, `kernel_ext.rs`); a file defining a new class takes the class name.

   **Types nest under a Module, not a Class.** Place new types at the top level (`Kobako::Capture`, `Kobako::Usage`) or under a Module (`Kobako::Transport::Request`, `Kobako::Outcome::Panic`); a stateful Class is per-instance and should not double as the namespace for sibling types.

3. **Keep it simple — in both directions.** Building: model exactly what SPEC requires — no speculative interfaces, parallel hierarchies, or defensive layers; three similar lines beats a premature abstraction; avoid feature flags and back-compat shims when the code can just change. Polishing: the feature set has converged, so actively converge the implementation — the ideal change keeps behavior identical with less implementation, zero external-surface change, and existing tests pinning it.

   **To prune safely, lock external interfaces first, then prune what sits behind them.** Worked example: the `Kobako::Outcome` migration (decode-boundary rename + lift → wire-format simplification → internal absorption), each step kept the previous step's external surface intact.

4. **Follow language community conventions via tooling.** Ruby: Rubocop + Steep. Rust: `cargo fmt` + `cargo clippy -D warnings` (also under `--target wasm32-wasip1`). Quality gating is two-stage, defined in `.claude/settings.json`: every Edit/Write runs the fast checks (rubocop autocorrect, cargo fmt, whole-project steep) and a failure blocks the edit; every Stop runs the full gate (clippy and rustdoc across all workspaces, then `bundle exec rake`). When a cop or lint fires, **shrink the code to fit the tool** — don't widen `.rubocop.yml` exclusions or add `#[allow]` / `# steep:ignore`.

   **Tool-vs-tool conflicts are the one justified widening.** When Rubocop and Steep / RBS upstream disagree on the same code shape, prefer the type-system guidance and disable the cop at the `.rubocop.yml` level with a comment citing the upstream source — worked example: `Style/DataInheritance` there.

5. **Document Ruby in RDoc prose.** Match the existing style — wrap identifiers in `+code+`, state intent rather than citing behavior anchors (Principle 1), no YARD tags (`@param` / `@return` / `@raise`); migrate stale tags when touching nearby code. Cross-references to another documented class or method are written bare (`TrapError`, `#read_usage!`, `ExtTypes#unpack_handle`) — RDoc auto-links resolvable names, and `{Ref}` braces or Markdown backticks suppress that linking and render literally, so neither is used; `+code+` marks identifiers that are not link targets (a bare lowercase `word?` does not auto-link — spell it `#word?`). `lib/kobako/catalog/handles.rb` is the worked example.

   **In Rust, wrap identifiers in backtick code spans (`` `Invocation` ``); do not use rustdoc intra-doc links (`` [`Invocation`] ``)** — they rot silently on renames and cannot target private items. The Stop hook gates this (`cargo doc -D warnings --document-private-items`, every workspace); reference-style file links to a format/contract doc (e.g. `[wire codec]: ../wire-codec.md`) are not intra-doc links and stay, but per Principle 1 these are doc pointers, not behavior-anchor citations.

6. **Docs and comments state intent in 1–2 sentences; don't explain mechanism.** A doc or comment block answers "what + why" — mechanism is what the code already shows. Drop chained rationale, generic SE narration, and grep-discoverable enumerations; in implementation comments a worked-example pointer (file path, named migration) replaces enumerated cases — behavior-anchor citations belong in tests and prose docs, not here (Principle 1). Applies to RDoc, Rust doc comments, `docs/*.md`, and CLAUDE.md itself: a list that will drift as the project evolves is written as intent plus a pointer to the gate or source that owns it; only SPEC-pinned, stable facts are enumerated.

7. **Route end-to-end coverage through the real mruby guest** (`data/kobako.wasm`). Do not introduce parallel fixture-driven wasm crates; if a behavior cannot be exercised through mruby, prefer a host-side unit test against `Kobako::Outcome` / `Kobako::Transport::Dispatcher` or a hand-rolled minimal wasm module (see `test/fixtures/minimal.wasm`).

8. **`test/` holds every Ruby test; `tasks/` holds no tests.** The suite's subject is gem runtime behavior — including cross-language integration (host↔guest fuzz, ABI invariants) — plus the tooling unit suites: `test/tasks/` covers the `tasks/support/` readers, `test/bench/` the `benchmark/support/` gate logic (grouped runs: `rake test:tasks` / `test:bench`). Build/packaging/lint/static-check *wrapper tasks* stay in `tasks/*.rake` or top-level scripts and never move into `test/`.

9. **Commit lock files.** Every workspace's `Cargo.lock` (root, `crates/`, `wasm/`) and `Gemfile.lock` ship alongside the dependency changes that produced them.

10. **Test assertion messages are contract statements, not implementation narrative.** Phrase each `assert_*` message as "<input shape> through <public API> must <observable behaviour>"; keep witness rationale in the comment block above the test method. The IO write coverage tests in `test/e2e/test_io_write.rb` are the worked correction example.

## Build Pipeline

The Guest Binary (`data/kobako.wasm`) is gitignored and built via a two-stage rake chain: `beni:build` (Stages A+B, owned by the beni gem against `build_config/wasi.rb` — mrbgem allowlist policy and toolchain rules are commented there) then `wasm:build` (Stage C, `tasks/wasm/build.rake` — including the non-obvious linker choice). Stage C ends with the `kobako-baker` bake (`wasm/kobako-baker`, host-side standalone crate): the canonical boot state (B-49) is pre-initialized into every shipped artifact, gated by a double-bake byte-identity check (F-10). `rake compile` from a clean clone walks the full chain and separately builds the native ext (`ext/kobako/` plus its `crates/` path dependencies — host-side `wasmtime` via `rb_sys`, not the guest).

The default `data/kobako.wasm` is pure (mruby + `kobako-io`); Regexp and JSON are opt-in capability variants built by `wasm:build:<variant>` and shipped as downloadable Release assets — composition rules and the variant matrix live in `docs/variants.md`. The gem bundles only the pure default.

CI (`.github/workflows/main.yml`) runs `bundle exec rake` — the default task (`compile + test + rubocop + steep + gate`) is the canonical gate, where `rake gate` runs the `gate:*` verification checks in `tasks/gate/`. `gate` enumerates them in one place so membership stays deliberate; the default and CI reference `gate`, never the list.

## Common Commands

Non-obvious entry points only — `rake -T` is the full catalog.

| Task | Command |
|------|---------|
| Default CI gate (compile + test + rubocop + steep + gate) | `bundle exec rake` |
| Run the release gate's `gate:*` verification checks | `rake gate` |
| Run one Ruby test file | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb` |
| Run one Ruby test by name | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb -n /pattern/` |
| Build native ext (`lib/kobako/kobako.bundle`) | `bundle exec rake compile` |
| Build Guest Binary (pure default, full chain) | `bundle exec rake wasm:build` |
| Build a capability variant (`regexp`, `regexp_unicode`, `json`, `full`) | `bundle exec rake wasm:build:<variant>` |
| Guest crate tests on the host (wasm32 has no test runner) | `bundle exec rake wasm:test` |
| Host crate unit tests (`crates/` workspace) | `bundle exec rake crates:test` |
| Clean Stage B / Stage C | `rake beni:clean` / `rake wasm:clean` |
| Clean vendor toolchains (keeps tarball cache) | `rake beni:vendor:clean` |
| Interactive REPL with gem loaded | `bin/console` |
| SPEC regression benchmarks (gated, #1..#6) | `bundle exec rake bench` |
| Characterization + release-gate bench tasks | `rake -T bench` lists them; gate workflow in `benchmark/README.md` |
| Code statistics (polish signal): per tier / per module / one module by language | `rake stats` · `rake stats:all` · `rake stats:<module>` (e.g. `stats:gem`, `stats:kobako-codec`; per-module tasks stay out of `rake -T`) |
| Line coverage, per language — Ruby `lib/` (stdlib Coverage), host + guest crates (`cargo llvm-cov`) | `rake coverage:ruby` · `coverage:crates` · `coverage:wasm` |
| Anchor citation profile + Pending gate (`docs/anchor-coverage.md`) | `rake gate:anchors:coverage` |
| Wire-symmetric peer inventories (`docs/wire-contract.md` § Wire-Symmetric Peers) | `rake gate:wire:symmetry` |
| Gem-sourced RBS pins match Gemfile.lock (run `rbs collection update` on drift) | `rake gate:rbs:lock` |
| Pub-surface acknowledgement ledger names only current pub items | `rake gate:surface` |
| Polish signals: churn hotspots / unconsumed pub surface (ledger consistency gated by `gate:surface`) | `rake stats:hotspots` / `rake stats:surface` |

## Layering

### The source trees across the wasm boundary

Host and guest meet at the wasm sandbox boundary and are deliberately **wire-symmetric**; `crates/kobako-wasmtime` is the driver that connects them, behind the `crates/kobako-runtime` contract, with `ext/` as the magnus shim on top.

```
HOST (process)                                  │  GUEST (wasm32, one Sandbox)
────────────────────────────────────────────── │ ──────────────────────────────────────
lib/  — Ruby gem, the user-facing API           │  wasm/kobako-wasm   — leaf shell (cdylib),
       ▲  owns the host wire codec              │    composes the guest crates into
       │  (#encode / .decode, duck-typed)       │    data/kobako.wasm
       │                                        │  wasm/kobako-mruby  — mruby implementation
       │                                        │  wasm/kobako-{io,regexp,json}
       │                                        │                     — capability gems
       │                                        │  wasm/kobako-core   — guest ABI contract
       │                                        │  crates/kobako-codec — shared wire tier
       │                                        │       ▲  owns the guest wire codec
       ▼                                        │       │  (codec::{Encode,Decode} trait)
ext/  — magnus shim over the crates/ driver     │  beni / beni-sys    — typed wrapper + FFI
crates/ — kobako-codec + kobako-runtime         │    (crates.io) → libmruby.a (mruby C API)
  + kobako-wasmtime (+ kobako, the Rust SDK)    │
       └─────────── drives the ABI ─── wasm ────┼───────┘
                    (alloc / eval / run / take_outcome / dispatch / yield)
```

- **`lib/` ↔ `crates/kobako-codec` are wire-symmetric peers.** Each independently implements the same SPEC wire so envelopes round-trip byte-for-byte (the `*_oracle` fuzz checks pin this); the one asymmetry that stays is the error model — success/failure is a value on the guest (`Outcome` enum) but return-or-raise on the host. `kobako-codec` lives in `crates/` because it is not wasm-only: the guest crates consume it across the workspace boundary, and a Rust embedder consumes it directly.
- **Five publishable guest crates, one shell, one bake tool.** Crate roles live in the stack diagram below; `wasm/kobako-wasm` is the unpublished cdylib-only shell composing them into `data/kobako.wasm`, the same path any third-party guest takes. `wasm/kobako-baker` (publishable, host-side, standalone `[workspace]` — wizer/wasmtime must never enter the wasm32 graph) bakes the canonical boot state into any kobako guest artifact. Published-crate internals follow the beni placeholder rule (compile on every target, fail at runtime); only `kobako-mruby` additionally mirrors the `mruby_linked` cfg in its build.rs.
- **The host's wasmtime driver is `crates/kobako-wasmtime`, not a wire endpoint.** It implements the `crates/kobako-runtime` contract, drives the ABI exports, and shuttles *raw bytes* between Ruby (which owns the codec) and the guest — never decodes envelopes. `ext/` is the magnus shim over it; the gem ships both crates as the ext's path-dependency closure (see the gemspec allowlist).
- **The typed mruby wrapper is the published `beni` crate** ([elct9620/beni](https://github.com/elct9620/beni)), consumed directly (`use beni::...`) by the guest crates; its `beni-sys` FFI layer discovers `libmruby.a` via `MRUBY_LIB_DIR` + `WASI_SDK_PATH` (exported by `rake wasm:build`). Wrapper-tier changes are beni contributions consumed here by a dependency bump — see beni's own CLAUDE.md for its layering and API surface.

### `lib/` tier stack

Dependencies point downward — a tier may use the tiers below it, never above. A tier maps to its directory (`lib/kobako/{codec,transport,catalog}/`); the flat `lib/kobako/*.rb` files split between Orchestration and Root by state and dependencies, not by a membership list.

```
Orchestration   stateful coordinators — Sandbox, Pool, Runtime (+ ext)
      │
Catalog         per-Sandbox registries (lib/kobako/catalog/)
      │
Transport ──┐   wire envelopes + dispatch (lib/kobako/transport/)
Outcome ────┤   guest-result decode (outcome.rb + outcome/)
      │     │
Codec ◄─────┘   byte-level wire (lib/kobako/codec/)
      │
Root            dependency-free value objects and error classes at Kobako::* —
                pure data / invariants, depend on nothing
```

**Placement rule (a `Codec → Transport` cycle bit us once):** a type's namespace follows **dependency direction, not which layer reads it most**. `Kobako::Handle` (ext 0x01) and `Kobako::Fault` (ext 0x02) are consumed almost entirely by Transport, yet sit at the root because `Codec` — below Transport — must register them; nesting them under `Transport` would force `Codec` to depend upward. When unsure, put the type at the **lowest tier that needs it**.

**Accepted lateral edge:** `Outcome` requires `transport/error.rb`. The `Kobako::Transport::Error` name is SPEC-pinned (SPEC.md "Wire-level error class"), so the class stays at its namespace path; the file itself depends only on root `errors.rb`, so the edge cannot close into a cycle. Do not relocate the definition to "fix" this.

**Per-operation codec state:** `Codec.forbid_faults` and `Codec.track_handles` are brackets over the Codec tier's private `State` — `forbid_faults` enforces that the Fault envelope (ext 0x02) is legal only in its envelope position, so every payload-position decode is wrapped in it (E-50). Brackets wrap **only the decode call**: a bracket spanning guest re-entry would leak the flag into nested operations.

### Host native stack (`ext/` + `crates/`)

The magnus surface lives only in `ext/kobako`; the engine mechanics live in `crates/kobako-wasmtime` behind the engine-free `crates/kobako-runtime` contract — the surface a non-Ruby host consumes. Both crates ship inside the gem as the ext's path dependencies (the `crates/` workspace manifest never ships, so member manifests use no `workspace = true` inheritance). The third `crates/` member, `kobako-codec`, is the wire tier: the ext never touches it (the wasmtime driver shuttles raw bytes; Ruby owns the host codec), so it stays outside the gem's crate closure.

**`crates/kobako` is the second frontend**: the bare-name Rust host SDK (`Sandbox` / `Receiver` glue over the same driver; released with the linked crate group under the `kobako-sdk` component). Its behavior alignment with `lib/` is pinned by the differential parity harness — `docs/parity.md` holds the mechanism and the CORE anchor manifest, `rake gate:parity:coverage` gates manifest coverage, and the unpublished `crates/kobako-parity` runner is the Rust executor. Ruby-parity is behavioral only; the SDK's API shape stays idiomatic Rust.

```
Ruby shim       ext/kobako — magnus surface only: the Kobako::Runtime class,
      │           dispatch-Proc GC root, neutral error channels → Kobako::* classes
Rust SDK        crates/kobako — idiomatic Rust host frontend over the same driver
      │           (Sandbox / Receiver / Yielder / Catalog; outcome parity-pinned)
Driver          crates/kobako-wasmtime — implements the Runtime contract, drives the
      │           ABI exports, per-invocation byte shuttle; process-wide Engine +
      │           Module cache, per-path InstancePre
Contract        crates/kobako-runtime — trait Runtime · DispatchHandler · Yielder
                  · Profile(declared isolation ladder) · Snapshot{Completion,
                  Capture, Usage} · Trap · SetupError  (engine-free, frontend-free)
```

Inside `kobako-wasmtime`, sibling modules reference each other as `crate::dispatch` / `crate::trap` (not `super::`).

### Guest crate stack (`wasm/`)

Mirrors `lib/` tier-for-tier — `crates/kobako-codec` is the wire-symmetric peer and `kobako-core` adds the guest-ABI machinery on top; `kobako-mruby` implements the contract over mruby; the cdylib-only `kobako-wasm` shell composes the published crates into `data/kobako.wasm`.

```
kobako-wasm     unpublished leaf shell (cdylib-only) — KobakoGuest wires the
      │           capability gems via init_gems; export_guest! emits the
      │           __kobako_* ABI exports
kobako-mruby    assembled mruby implementation (publishable rlib) — MrbGuest trait
      │           (required init_gems hook; provided eval / run / yield flows),
      │           per-invocation entry flows, Kobako runtime bridge, mrb ↔ wire
      │           value conversion
kobako-io / kobako-regexp / kobako-json
      │         capability gems (publishable rlibs, kobako-mruby-free) — pure-Rust
      │           beni::Gem impls over wasi-libc write(2) / fancy-regex / serde_json
kobako-core     guest ABI contract (publishable rlib, mruby-free) — Guest trait +
      │           export_guest!, ABI primitives (outcome buffer, frames),
      │           transport::proxy driving __kobako_dispatch
kobako-codec    portable wire tier (publishable rlib in crates/, mruby- and
                engine-free) — codec + transport envelopes + outcome, the
                wire-symmetric peer of lib/

(mruby)         beni (typed wrapper) → beni-sys (bindgen FFI) — crates.io;
                consumed by the mruby-linked guest crates
```

## Where to Look

Entry points only — siblings are reachable from there. Notes carry only what reading the entry-point file won't tell you.

| Topic | Entry points | Notes |
|-------|--------------|-------|
| Wire format / codec | host `lib/kobako/codec/`, `lib/kobako/transport/`; Rust side `crates/kobako-codec/src/{codec.rs,transport/}` | Envelope shapes: `docs/wire-contract.md`. Byte-level: `docs/wire-codec.md`. Ext-type leaves are root-level: `Kobako::Handle` (0x01), `Kobako::Fault` (0x02). |
| Error taxonomy / outcome | `lib/kobako/errors.rb`, `lib/kobako/outcome.rb` | E-xx anchors in `docs/behavior/errors.md`. |
| Sandbox lifecycle | host `lib/kobako/sandbox.rb`, `crates/kobako-wasmtime/src/driver.rs` (magnus shim: `ext/kobako/src/runtime.rs`); guest `wasm/kobako-mruby/src/flows.rs` | `Kobako::Transport::Run` carries the `#run` host→guest envelope; guest→host dispatch arrives via `Runtime#on_dispatch=` Proc (`lib/kobako/transport/dispatcher.rb`). B-xx in `docs/behavior/lifecycle.md` and `invocation.md`. |
| Guest IO / `$stdout` / `$stderr` | `wasm/kobako-io/src/{io,kernel_ext}.rs` | Pure-Rust `beni::Gem` (no mrblib / mrbc pipeline, no `beni::sys`); Kernel delegators registered private via `Module::define_private_method`. SPEC B-04. |
| Guest Regexp / MatchData | `wasm/kobako-regexp/src/{regexp,matchdata,translate}.rs` | Pure-Rust `beni::Gem` over `fancy-regex`; byte-based offsets; `translate.rs` rewrites Ruby `\d\w\s`→ASCII + flag mapping. SPEC B-41; per-behavior RX-xx anchors in `docs/regexp.md`. |
| Guest JSON | `wasm/kobako-json/src/{json,convert,errors}.rs` | Pure-Rust `beni::Gem` over `serde_json`; `Object#as_json` opt-in that parse can't use to forge a Handle. SPEC B-52 / B-53; per-behavior JS-xx anchors in `docs/json.md`. |
| Transport dispatch | host `lib/kobako/transport/dispatcher.rb`; guest `wasm/kobako-core/src/transport/` | Host dispatcher **never raises** — every failure becomes a `Response.err` envelope. |
| Catalog::Handles / capability handles | `lib/kobako/catalog/handles.rb` | B-12..B-21 in `docs/behavior/dispatch.md`. Owned by Sandbox (B-19), injected into `Kobako::Catalog::Services` so guest→host dispatch and host→guest wire encoding share one allocator. Per-invocation reset is the Sandbox's job. |
| Service registration | `lib/kobako/catalog/services.rb` | B-08..B-11 in `docs/behavior/registration.md`. Per-Sandbox `Catalog::Services` holds the flat path→Service bindings; a Service is bound at a constant-path name (`"MyService::KV"`, a deeper `"MyService::Nested::KV"`, or a top-level `"File"`). |
| Extension installation (`#install`) | host `lib/kobako/extension.rb`, `lib/kobako/catalog/extensions.rb`; SDK `crates/kobako/src/extension.rs` | B-55..B-57 / E-51..E-53 in `docs/behavior/extension.md`; contract + File example in `docs/extensions.md`. Composes a guest idiom (`source`) with an optional host backend over `#preload` + `#bind` — a callable provider refreshes per invocation (Ruby `Catalog::Services#refresh`, Rust dispatch overlay). kobako ships no concrete Extension. |
| Security model / reflection denial | `docs/security-model.md` (host guidance, not a SPEC contract); anchors in `docs/behavior/security.md` | Guest-side rejection mirrors are non-authoritative; the host is the boundary. |
| Guest Binary variants | `docs/variants.md`, `tasks/wasm/build.rake` | Variant matrix and composition rules. |
| ABI surface (host ↔ guest exports) | contract `wasm/kobako-core/src/guest.rs` (`Guest` + `export_guest!`); entry bodies `wasm/kobako-mruby/src/flows.rs` ↔ `crates/kobako-wasmtime/src/driver.rs` | — |
| E2E coverage | `test/e2e/` (`#eval`, one file per behaviour group), `test/sandbox/test_run.rb` (`#run`) | Both drive real `data/kobako.wasm`. Wrapper-tier (`test/runtime/test_runtime.rb`) covers only `from_path`. |
| Ruby↔Rust parity harness | `docs/parity.md`, `test/parity/` + `test/support/parity/`, `crates/kobako` + `crates/kobako-parity` | Differential: one scenario, two frontends, normalized observables compared. CORE manifest in the doc; `rake gate:parity:coverage` gates it. |
| mruby typed wrapper / FFI | `beni` + `beni-sys` crates ([elct9620/beni](https://github.com/elct9620/beni)) | Consumed directly by the guest crates (`use beni::...`; raw FFI via the `beni::sys` re-export). Wrapper changes are beni contributions, pulled in by a dependency bump. |
| RBS signatures | `sig/kobako/` (mirrors `lib/kobako/` 1:1) | Three sources stack: `sig/_external/` (hand-rolled), `rbs_collection.{yaml,lock.yaml}` (gem), `library "<name>"` in `Steepfile` (stdlib — reach for first). PostToolUse steep hook blocks Ruby edits without matching `.rbs`. |
| Regression benchmarks | `tasks/bench/`, `benchmark/` | #1..#6 gated (+10% regression blocks release); the rest are characterization, not gated. Results: `benchmark/results/<date>-<short-sha>.json`. |
| Build / toolchain | Rakefile (`Beni::Tasks` block), `build_config/wasi.rb`, `tasks/wasm/` | Stages A+B live in the beni gem (`rake beni:build`); kobako keeps only the build config and Stage C. |

`test/test_helper.rb` rescues `LoadError` when `lib/kobako/kobako.bundle` is missing and stubs `Kobako::Error`, so the suite still loads on a clean checkout; individual tests `skip` themselves when the native ext is absent.
