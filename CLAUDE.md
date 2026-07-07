# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

kobako is a Ruby gem that provides an in-process Wasm sandbox for running untrusted mruby scripts. The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing the mruby interpreter; host and guest communicate over a MessagePack-based Transport wire defined in `SPEC.md`.

## Principles

Apply these in order — earlier principles override later ones on conflict.

1. **SPEC.md is the source of truth.** Behavior contracts live in `SPEC.md` or in the `docs/<topic>.md` it indexes; the per-anchor behavior detail lives in `docs/behavior/<aspect>.md`, mapped from each anchor range by the grouping table in `SPEC.md` § Behavior. B-xx / E-xx are append-only, never renumbered, and the `rake anchors` gate enforces uniqueness, contiguity, and resolvability across the spec corpus. **Cite anchors only where traceability belongs: in tests (the test that verifies an anchor names it) and in prose docs (linking directly to the `docs/behavior/<aspect>.md` that holds it). Implementation and public-API comments state intent — what + why — not anchor citations.** When SPEC is silent, extend it (or the relevant `docs/<topic>.md`) first.

2. **One thing per file; keep files small.** Split a growing module into a façade plus per-responsibility files in a sibling directory — `Kobako::Transport` and `Kobako::Snippet` are the worked examples. In guest capability gems, a file extending an existing core class takes the `_ext` suffix (`string_ext.rs`, `kernel_ext.rs`); a file defining a new class takes the class name.

   **Types nest under a Module, not a Class.** Place new types at the top level (`Kobako::Capture`, `Kobako::Usage`) or under a Module (`Kobako::Transport::Request`, `Kobako::Outcome::Panic`); a stateful Class is per-instance and should not double as the namespace for sibling types.

3. **Keep it simple. Don't pre-abstract.** Model exactly what SPEC requires — no speculative interfaces, parallel hierarchies, or defensive layers. Three similar lines beats a premature abstraction; avoid feature flags and back-compat shims when the code can just change.

4. **Follow language community conventions via tooling.** Ruby: Rubocop + Steep. Rust: `cargo fmt` + `cargo clippy -D warnings` (also under `--target wasm32-wasip1`). All four run on every Edit/Write via PostToolUse hooks; failures block the edit. When a cop or lint fires, **shrink the code to fit the tool** — don't widen `.rubocop.yml` exclusions or add `#[allow]` / `# steep:ignore`.

   **Tool-vs-tool conflicts are the one justified widening.** When Rubocop and Steep / RBS upstream disagree on the same code shape, prefer the type-system guidance and disable the cop at the `.rubocop.yml` level with a comment citing the upstream source — worked example: `Style/DataInheritance` there.

5. **Document Ruby in RDoc prose.** Match the existing style — wrap identifiers in `+code+`, state intent rather than citing behavior anchors (Principle 1), no YARD tags (`@param` / `@return` / `@raise`); migrate stale tags when touching nearby code. Cross-references to another documented class or method are written bare (`TrapError`, `#read_usage!`, `Factory#unpack_handle`) — RDoc auto-links resolvable names, and `{Ref}` braces or Markdown backticks suppress that linking and render literally, so neither is used; `+code+` marks identifiers that are not link targets (a bare lowercase `word?` does not auto-link — spell it `#word?`). `lib/kobako/catalog/handles.rb` is the worked example.

   **In Rust, wrap identifiers in backtick code spans (`` `Invocation` ``); do not use rustdoc intra-doc links (`` [`Invocation`] ``)** — they rot silently on renames and cannot target private items. The Stop hook gates this (`cargo doc -D warnings --document-private-items`, every workspace); reference-style file links to a format/contract doc (e.g. `[wire codec]: ../wire-codec.md`) are not intra-doc links and stay, but per Principle 1 these are doc pointers, not behavior-anchor citations.

6. **Docs and comments state intent in 1–2 sentences; don't explain mechanism.** A doc or comment block answers "what + why" — mechanism is what the code already shows. Drop chained rationale, generic SE narration, and grep-discoverable enumerations; in implementation comments a worked-example pointer (file path, named migration) replaces enumerated cases — behavior-anchor citations belong in tests and prose docs, not here (Principle 1). Applies to RDoc, Rust doc comments, `docs/*.md`, and CLAUDE.md itself.

7. **Route end-to-end coverage through the real mruby guest** (`data/kobako.wasm`). Do not introduce parallel fixture-driven wasm crates; if a behavior cannot be exercised through mruby, prefer a host-side unit test against `Kobako::Outcome` / `Kobako::Transport::Dispatcher` or a hand-rolled minimal wasm module (see `test/fixtures/minimal.wasm`).

8. **`test/` holds gem runtime behavior only.** Build/packaging/lint/static-check wrappers belong in `tasks/*.rake` or top-level scripts. Cross-language integration tests (host↔guest fuzz, ABI invariants) do belong in `test/`.

9. **Commit lock files.** Every workspace's `Cargo.lock` (root, `crates/`, `wasm/`) and `Gemfile.lock` ship alongside the dependency changes that produced them.

10. **Lock external interfaces before pruning internals.** Settle the outward-facing API first, then prune what sits behind it. Worked example: the `Kobako::Outcome` migration (decode-boundary rename + lift → wire-format simplification → internal absorption), each step kept the previous step's external surface intact.

11. **Test assertion messages are contract statements, not implementation narrative.** Phrase each `assert_*` message as "<input shape> through <public API> must <observable behaviour>"; keep witness rationale in the comment block above the test method. The IO write coverage tests in `test/e2e/test_io_write.rb` are the worked correction example.

## Build Pipeline

The Guest Binary (`data/kobako.wasm`) is gitignored and built via a two-stage rake chain: `beni:build` (Stages A+B, owned by the beni gem against `build_config/wasi.rb` — mrbgem allowlist policy and toolchain rules are commented there) then `wasm:build` (Stage C, `tasks/wasm/build.rake` — including the non-obvious linker choice). Stage C ends with the `kobako-baker` bake (`wasm/kobako-baker`, host-side standalone crate): the canonical boot state (B-49) is pre-initialized into every shipped artifact, gated by a double-bake byte-identity check (F-10). `rake compile` from a clean clone walks the full chain and separately builds the native ext (`ext/kobako/` plus its `crates/` path dependencies — host-side `wasmtime` via `rb_sys`, not the guest).

The default `data/kobako.wasm` is pure (mruby + `kobako-io`); Regexp and JSON are opt-in. `wasm:build:regexp` and `wasm:build:regexp_unicode` compose the `kobako-regexp` gem under the shell's `regexp` / `regexp-unicode` cargo features into `data/kobako+regexp{,-unicode}.wasm`; `wasm:build:json` and `wasm:build:full` compose the `kobako-json` gem alone, or together with ASCII regexp, into `data/kobako+json.wasm` / `data/kobako+full.wasm`. The gem bundles only the pure default; the variants ship as downloadable Release assets.

CI (`.github/workflows/main.yml`) runs `bundle exec rake` — the default task (`compile + test + rubocop + steep + anchors + parity:coverage`) is the canonical gate.

## Common Commands

| Task | Command |
|------|---------|
| Default CI task (compile + test + rubocop + steep) | `bundle exec rake` |
| Run steep type check only | `bundle exec rake steep` |
| Build native ext (`lib/kobako/kobako.bundle`) | `bundle exec rake compile` |
| Build Guest Binary (pure default, full chain) | `bundle exec rake wasm:build` |
| Build regexp variants | `bundle exec rake wasm:build:regexp wasm:build:regexp_unicode` |
| Run all Ruby tests | `bundle exec rake test` |
| Run one Ruby test file | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb` |
| Run one Ruby test by name | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb -n /pattern/` |
| Guest crate host-only tests (wasm32 has no test runner) | `bundle exec rake wasm:test` |
| Guest crate `cargo check` | `bundle exec rake wasm:check` |
| Host crate unit tests (`crates/` workspace) | `bundle exec rake crates:test` |
| Clean Stage B / Stage C | `rake beni:clean` / `rake wasm:clean` |
| Clean vendor toolchains | `rake beni:vendor:clean` (keeps tarball cache) or `rake beni:vendor:clobber` |
| Interactive REPL with gem loaded | `bin/console` |
| SPEC regression benchmarks (#1..#6, ≤1 MiB payloads) | `bundle exec rake bench` |
| Regression benchmarks + 16 MiB codec sweep | `bundle exec rake bench:full` |
| Concurrent characterization (#7, not gated) | `bundle exec rake bench:concurrent` |
| Memory characterization (#8, not gated) | `bundle exec rake bench:memory` |

## Layering

### The source trees across the wasm boundary

Host and guest meet at the wasm sandbox boundary and are deliberately **wire-symmetric**; `crates/kobako-wasmtime` is the driver that connects them, behind the `crates/kobako-runtime` contract, with `ext/` as the magnus shim on top.

```
HOST (process)                                  │  GUEST (wasm32, one Sandbox)
────────────────────────────────────────────── │ ──────────────────────────────────────
lib/  — Ruby gem, the user-facing API           │  wasm/kobako-wasm  — leaf shell (cdylib)
  Sandbox · Catalog · Transport · Outcome        │    KobakoGuest (impl MrbGuest) +
  · Codec · Root        (tier stack below)       │    export_guest! → data/kobako.wasm
       ▲  owns the wire codec                    │  wasm/kobako-mruby — assembled mruby impl
       │  (#encode / .decode, duck-typed)        │    MrbGuest(flows) · Kobako(runtime)
       │                                         │    · KobakoBridge gem · codec convert
       │                                         │  wasm/kobako-io  — IO / Kernel gem
       │                                         │    ::IO · $stdout/$stderr · delegators
       │                                         │  wasm/kobako-regexp  — Regexp / MatchData gem
       │                                         │    Regexp · MatchData · String integ
       │                                         │  wasm/kobako-json  — JSON gem
       │                                         │    JSON.generate/parse · Object#as_json
       │                                         │  wasm/kobako-core  — guest ABI contract
       │                                         │    Guest trait · abi(primitives) · frames
       │                                         │    · transport::proxy
       │                                         │  crates/kobako-codec  — shared wire tier
       │                                         │    codec · envelopes · outcome (mirrors lib/)
       │                                         │       ▲  owns the wire codec
       │                                         │       │  (codec::{Encode,Decode} trait)
       │                                         │  beni / beni-sys  — typed wrapper + FFI
       │                                         │    (crates.io) → libmruby.a (mruby C API)
       ▼                                         │       ▲  kobako-wasm → kobako-mruby · kobako-io ·
ext/  — magnus shim over the crates/ driver      │       │    kobako-regexp · kobako-json · core · codec;
  runtime.rs shell · bridge · errors             │       │    kobako-mruby → core · codec · beni;
crates/ — kobako-codec + kobako-runtime          │       │    io/-regexp/-json → beni
  + kobako-wasmtime                              │       │
       └─────────── drives the ABI ─── wasm ─────┼───────┘
                    (alloc / eval / run / take_outcome / dispatch / yield)
```

- **`lib/` ↔ `crates/kobako-codec` are wire-symmetric peers.** Each independently implements the same SPEC wire so envelopes round-trip byte-for-byte (the `*_oracle` fuzz checks pin this); the one asymmetry that stays is the error model — success/failure is a value on the guest (`Outcome` enum) but return-or-raise on the host. `kobako-codec` lives in `crates/` because it is not wasm-only: the guest crates consume it across the workspace boundary, and a Rust embedder consumes it directly.
- **Five publishable guest crates, one shell, one bake tool.** Crate roles live in the stack diagram below; `wasm/kobako-wasm` is the unpublished cdylib-only shell composing them into `data/kobako.wasm`, the same path any third-party guest takes. `wasm/kobako-baker` (publishable, host-side, standalone `[workspace]` — wizer/wasmtime must never enter the wasm32 graph) bakes the canonical boot state into any kobako guest artifact. Published-crate internals follow the beni placeholder rule (compile on every target, fail at runtime); only `kobako-mruby` additionally mirrors the `mruby_linked` cfg in its build.rs.
- **The host's wasmtime driver is `crates/kobako-wasmtime`, not a wire endpoint.** It implements the `crates/kobako-runtime` contract, drives the ABI exports, and shuttles *raw bytes* between Ruby (which owns the codec) and the guest — never decodes envelopes. `ext/` is the magnus shim over it; the gem ships both crates as the ext's path-dependency closure (see the gemspec allowlist).
- **The typed mruby wrapper is the published `beni` crate** ([elct9620/beni](https://github.com/elct9620/beni)), consumed directly (`use beni::...`) by the guest crates; its `beni-sys` FFI layer discovers `libmruby.a` via `MRUBY_LIB_DIR` + `WASI_SDK_PATH` (exported by `rake wasm:build`). Wrapper-tier changes are beni contributions consumed here by a dependency bump — see beni's own CLAUDE.md for its layering and API surface.

### `lib/` tier stack

Dependencies point downward — a tier may use the tiers below it, never above. The non-obvious tier is the **root** of dependency-free value objects: they live at `Kobako::*`, *not* under the layer that consumes them, so a lower layer can use them without an upward dependency.

```
Orchestration   Kobako::Pool, Kobako::Sandbox, Kobako::Runtime (+ ext)
      │
Catalog         Kobako::Catalog::{Namespaces, Snippets, Handles}
      │
Transport ──┐   Kobako::Transport::{Request, Response, Run, Yield, Dispatcher, Yielder}
Outcome ────┤   Kobako::Outcome (decode + Panic)
      │     │
Codec ◄─────┘   Kobako::Codec::{Encoder, Decoder, Factory, Utils, HandleWalk}   (byte-level wire)
      │
Root            Kobako::{Handle, Fault, Capture, Usage, Namespace, SandboxOptions},
                Kobako::Snippet::{Source, Binary}, Kobako::Outcome::Panic, Kobako::*Error
                — pure data / invariants, depend on nothing
```

**Placement rule (a `Codec → Transport` cycle bit us once):** a type's namespace follows **dependency direction, not which layer reads it most**. `Kobako::Handle` (0x01) and `Kobako::Fault` (0x02) are consumed almost entirely by Transport, yet sit at the root because `Codec` — below Transport — must register them; nesting them under `Transport` would force `Codec` to depend upward. When unsure, put the type at the **lowest tier that needs it**.

**Accepted lateral edge:** `Outcome` requires `transport/error.rb`. The `Kobako::Transport::Error` name is SPEC-pinned (SPEC.md "Wire-level error class"), so the class stays at its namespace path; the file itself depends only on root `errors.rb`, so the edge cannot close into a cycle. Do not relocate the definition to "fix" this.

### Host native stack (`ext/` + `crates/`)

The magnus surface lives only in `ext/kobako`; the engine mechanics live in `crates/kobako-wasmtime` behind the engine-free `crates/kobako-runtime` contract — the surface a non-Ruby host consumes. Both crates ship inside the gem as the ext's path dependencies (the `crates/` workspace manifest never ships, so member manifests use no `workspace = true` inheritance). The third `crates/` member, `kobako-codec`, is the wire tier: the ext never touches it (the wasmtime driver shuttles raw bytes; Ruby owns the host codec), so it stays outside the gem's crate closure.

**`crates/kobako` is the second frontend**: the bare-name Rust host SDK (`Sandbox` / `Member` glue over the same driver; released with the linked crate group under the `kobako-sdk` component). Its behavior alignment with `lib/` is pinned by the differential parity harness — `docs/parity.md` holds the mechanism and the CORE anchor manifest, `rake parity:coverage` gates manifest coverage, and the unpublished `crates/kobako-parity` runner is the Rust executor. Ruby-parity is behavioral only; the SDK's API shape stays idiomatic Rust.

```
Ruby shim       ext/kobako — runtime.rs (Kobako::Runtime class, dispatch-Proc GC
      │           root, per-outcome usage / capture readouts) · runtime/bridge.rs
      │           (RubyDispatchHandler + GuestYielder) · runtime/errors.rs
      │           (neutral channels → Kobako::* classes)
      │
Rust SDK        crates/kobako — Sandbox(seal-once eval/run/preload) · Member/
      │           Fault seam (+respond_to_guest narrowing) · Block(frame-
      │           borrowed yield channel) · Handles(per-invocation
      │           capability table) · CatalogHandler(never-fail dispatch)
      │           · snippet table · outcome classification (parity-pinned)
      │
Driver          crates/kobako-wasmtime — Driver (impl Runtime) + engine mechanics
      │           driver (caps bracket, ABI probe) · dispatch (__kobako_dispatch)
      │           · guest_mem (Caller alloc/write/read + CallerYielder) · trap
      │           (wasmtime error → neutral Trap) · capture · ambient (B-45)
      │           · instance_pre (Linker + per-path InstancePre cache)
      │           · invocation / exports / config (per-Store state) · cache
      │           (process-wide Engine + Module + epoch ticker)
      │
Contract        crates/kobako-runtime — trait Runtime · DispatchHandler · Yielder
                  · Profile(declared isolation ladder) · Snapshot{Completion,
                  Capture, Usage} · Trap · SetupError  (engine-free, frontend-free)
```

Inside `kobako-wasmtime`, sibling modules reference each other as `crate::dispatch` / `crate::trap` (not `super::`).

### Guest crate stack (`wasm/`)

Mirrors `lib/` tier-for-tier — `crates/kobako-codec` is the wire-symmetric peer and `kobako-core` adds the guest-ABI machinery on top; the `kobako-mruby` crate implements the contract over mruby; the cdylib-only `kobako-wasm` shell composes the published crates into `data/kobako.wasm`.

```
kobako-wasm  (unpublished leaf shell, cdylib-only)
Shell           guest — KobakoGuest (impl kobako_mruby::MrbGuest; init_gems
                  wires kobako-io + kobako-regexp + kobako-json) + Guest forwarding + export_guest!
                  emits __kobako_{eval,run,alloc,take_outcome,yield_to_block,abi_version}
────────────────────────────────────────────────────────────────────
kobako-mruby  (assembled mruby implementation — publishable rlib)
Harness         MrbGuest trait — required init_gems hook; provided
      │           eval / run / yield_to_block flows
Flows           flows + flows/{boot, eval, run, yield_block, snippets,
      │           mrb_slot} — per-invocation entry bodies; snippets =
      │           Frame 3 mruby payload semantics (source / RITE)
Runtime         runtime (Kobako token) + runtime/{init (KobakoBridge),
                  bridges, block_stack, codec_convert} — installs the
                  Kobako module / classes on an mrb_state; mrb ↔ wire
                  value conversion; per-invocation block stack
────────────────────────────────────────────────────────────────────
kobako-io  (IO / Kernel capability gem — publishable rlib, kobako-mruby-free)
                KobakoIo (impl beni::Gem) — ::IO class, STDOUT/STDERR,
                $stdout/$stderr, private Kernel delegators; pure Rust
                over wasi-libc write(2)
────────────────────────────────────────────────────────────────────
kobako-regexp  (Regexp / MatchData capability gem — publishable rlib, kobako-mruby-free)
                KobakoRegexp (impl beni::Gem) — Regexp · MatchData ·
                RegexpError · String integ (=~/match/scan/gsub/sub/
                split/[]/index); pure Rust over fancy-regex, translate
                Ruby pattern/flags → regex dialect
────────────────────────────────────────────────────────────────────
kobako-json  (JSON capability gem — publishable rlib, kobako-mruby-free)
                KobakoJson (impl beni::Gem) — JSON module
                (parse/generate/pretty_generate) · Object#as_json opt-in
                · JSON error tree; pure Rust over serde_json
────────────────────────────────────────────────────────────────────
kobako-core  (guest ABI contract crate — publishable rlib, mruby-free)
Contract        Guest trait (eval / run / yield_to_block) + export_guest! macro
      │
ABI primitives  abi — __kobako_dispatch import · pack/unpack_u64 ·
      │           alloc / take_outcome / write_outcome / write_panic (outcome buffer)
      │         frames — stdin channel reader + Frame 1 preamble parser
Dispatch        transport::proxy — drives __kobako_dispatch over the envelopes
────────────────────────────────────────────────────────────────────
kobako-codec  (portable wire tier — publishable rlib in crates/, mruby- and engine-free)
Transport ──┐   transport::{Request, Response, Yield}
Outcome ────┤   outcome::{Outcome, Panic}
      │     │
Codec ◄─────┘   codec::{Encoder, Decoder, Value, Error, Encode, Decode}   (byte-level wire)

(mruby)         beni (typed wrapper) → beni-sys (bindgen FFI) — crates.io;
                consumed by kobako-mruby, kobako-io, kobako-regexp, and kobako-json
```

## Where to Look

Entry points only — siblings (`outcome/panic.rb`, `snippet/{source,binary}.rb`, `transport/request.rb`, etc.) are reachable from there. Notes carry only what reading the entry-point file won't tell you.

| Topic | Entry points | Notes |
|-------|--------------|-------|
| Wire format / codec | host `lib/kobako/codec/`, `lib/kobako/transport/` (envelopes: `request.rb` / `response.rb` / `run.rb` / `yield.rb`); Rust side `crates/kobako-codec/src/{codec.rs,transport/}` | Envelope shapes: `docs/wire-contract.md`. Byte-level: `docs/wire-codec.md`. Ext-type leaves are root-level: `Kobako::Handle` (0x01), `Kobako::Fault` (0x02). |
| Error taxonomy / outcome | `lib/kobako/errors.rb`, `lib/kobako/outcome.rb` | E-xx anchors in `docs/behavior/errors.md`. |
| Sandbox lifecycle | host `lib/kobako/sandbox.rb`, `crates/kobako-wasmtime/src/driver.rs` (magnus shim: `ext/kobako/src/runtime.rs`); guest `wasm/kobako-mruby/src/flows.rs` | `Kobako::Transport::Run` carries the `#run` host→guest envelope; guest→host dispatch arrives via `Runtime#on_dispatch=` Proc (`lib/kobako/transport/dispatcher.rb`). B-xx in `docs/behavior/lifecycle.md` and `invocation.md`. |
| Guest IO / `$stdout` / `$stderr` | `wasm/kobako-io/src/{io,kernel_ext}.rs` | Pure-Rust `beni::Gem` (no mrblib / mrbc pipeline, no `beni::sys`); Kernel delegators registered private via `Module::define_private_method`. SPEC B-04. |
| Guest Regexp / MatchData | `wasm/kobako-regexp/src/{regexp,matchdata,string_ext,translate}.rs` | Pure-Rust `beni::Gem` over `fancy-regex`; self-defines `RegexpError`; byte-based offsets; `translate.rs` rewrites Ruby `\d\w\s`→ASCII + flag mapping. SPEC B-41. |
| Guest JSON | `wasm/kobako-json/src/{json,convert,errors}.rs` | Pure-Rust `beni::Gem` over `serde_json`; `JSON.parse` / `generate` / `pretty_generate`; `Object#as_json` opt-in that parse can't use to forge a Handle. SPEC B-52 / B-53. |
| Transport dispatch | host `lib/kobako/transport/dispatcher.rb`; guest `wasm/kobako-core/src/transport/` | Host dispatcher **never raises** — every failure becomes a `Response.err` envelope. |
| Catalog::Handles / capability handles | `lib/kobako/catalog/handles.rb` | B-13..B-21 in `docs/behavior/dispatch.md`. Owned by Sandbox (B-19), injected into `Kobako::Catalog::Namespaces` so guest→host dispatch and host→guest wire encoding share one allocator. Per-invocation reset is the Sandbox's job. |
| Service registration | `lib/kobako/catalog/namespaces.rb`, `lib/kobako/namespace.rb` | Per-Sandbox `Catalog::Namespaces` owns the `Kobako::Namespace` registry; bound objects live at `"<Namespace>::<Member>"` (e.g., `"MyService::KV"`). |
| ABI surface (host ↔ guest exports) | contract `wasm/kobako-core/src/guest.rs` (`Guest` + `export_guest!`); entry bodies `wasm/kobako-mruby/src/flows.rs` ↔ `crates/kobako-wasmtime/src/driver.rs` | — |
| E2E coverage | `test/e2e/` (`#eval`, one file per behaviour group), `test/sandbox/test_run.rb` (`#run`) | Both drive real `data/kobako.wasm`. Wrapper-tier (`test/runtime/test_runtime.rb`) covers only `from_path`. |
| Ruby↔Rust parity harness | `docs/parity.md`, `test/parity/` + `test/support/parity/`, `crates/kobako` + `crates/kobako-parity` | Differential: one scenario, two frontends, normalized observables compared. CORE manifest in the doc; `rake parity:coverage` gates it. |
| mruby typed wrapper / FFI | `beni` + `beni-sys` crates ([elct9620/beni](https://github.com/elct9620/beni)) | Consumed directly by the guest crates (`use beni::...`; raw FFI via the `beni::sys` re-export). Wrapper changes are beni contributions, pulled in by a dependency bump. |
| RBS signatures | `sig/kobako/` (mirrors `lib/kobako/` 1:1) | Three sources stack: `sig/_external/` (hand-rolled), `rbs_collection.{yaml,lock.yaml}` (gem), `library "<name>"` in `Steepfile` (stdlib — reach for first). PostToolUse steep hook blocks Ruby edits without matching `.rbs`. |
| Regression benchmarks | `tasks/bench/`, `benchmark/` | #1..#6 gated (+10% regression blocks release); #7..#10 characterization, not gated. Results: `benchmark/results/<date>-<short-sha>.json`. |
| Build / toolchain | Rakefile (`Beni::Tasks` block), `build_config/wasi.rb`, `tasks/wasm/` | Stages A+B live in the beni gem (`rake beni:build`); kobako keeps only the build config and Stage C. |

`test/test_helper.rb` rescues `LoadError` when `lib/kobako/kobako.bundle` is missing and stubs `Kobako::Error`, so the suite still loads on a clean checkout; individual tests `skip` themselves when the native ext is absent.
