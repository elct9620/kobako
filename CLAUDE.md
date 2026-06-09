# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

kobako is a Ruby gem that provides an in-process Wasm sandbox for running untrusted mruby scripts. The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing the mruby interpreter; host and guest communicate over a MessagePack-based Transport wire defined in `SPEC.md`.

## Principles

Apply these in order — earlier principles override later ones on conflict.

1. **SPEC.md is the source of truth.** Behavior contracts live in `SPEC.md` or in the `docs/<topic>.md` it indexes; cite anchors as `{SPEC.md B-04}[link:../../SPEC.md]` (B-xx / E-xx are append-only, never renumbered). When SPEC is silent, extend it (or the relevant `docs/<topic>.md`) first, then cite the new anchor.

2. **One thing per file; keep files small.** Split a growing module into a façade plus per-responsibility files in a sibling directory — `Kobako::Transport` and `Kobako::Snippet` are the worked examples.

   **Types nest under a Module, not a Class.** Place new types at the top level (`Kobako::Capture`, `Kobako::Snapshot`) or under a Module (`Kobako::Transport::Request`, `Kobako::Outcome::Panic`); a stateful Class is per-instance and should not double as the namespace for sibling types.

3. **Keep it simple. Don't pre-abstract.** Model exactly what SPEC requires — no speculative interfaces, parallel hierarchies, or defensive layers. Three similar lines beats a premature abstraction; avoid feature flags and back-compat shims when the code can just change.

4. **Follow language community conventions via tooling.** Ruby: Rubocop + Steep. Rust: `cargo fmt` + `cargo clippy -D warnings` (also under `--target wasm32-wasip1`). All four run on every Edit/Write via PostToolUse hooks; failures block the edit. When a cop or lint fires, **shrink the code to fit the tool** — don't widen `.rubocop.yml` exclusions or add `#[allow]` / `# steep:ignore`.

   **Tool-vs-tool conflicts are the one justified widening.** When Rubocop and Steep / RBS upstream disagree on the same code shape, prefer the type-system guidance and disable the cop at the `.rubocop.yml` level with a comment citing the upstream source. Worked example: `Style/DataInheritance` is disabled because ruby/rbs [`docs/data_and_struct.md`](https://github.com/ruby/rbs/blob/master/docs/data_and_struct.md) documents `class X < Data.define(...)` as the Steep-friendly pattern — every `Data.define` type in `lib/` uses the subclass form.

5. **Document Ruby in RDoc prose.** Match the existing style — wrap identifiers in `+code+`, cite SPEC as `{SPEC.md B-XX}[link:<relative path>]` in plain text, no YARD tags (`@param` / `@return` / `@raise`); migrate stale tags when touching nearby code.

   ```ruby
   # Host-side mapping from opaque integer Handle IDs to Ruby objects.
   # One table is owned per Sandbox and injected into Kobako::Catalog::Namespaces.
   # See {SPEC.md B-15}[link:../../../SPEC.md].
   class Handler
     # Bind +object+ and return its newly-allocated Handle ID.
     # Raises +Kobako::HandlerExhaustedError+ if the next ID would exceed +MAX_ID+.
     def alloc(object)
       # ...
     end
   end
   ```

   **In Rust, wrap identifiers in backtick code spans (`` `Invocation` ``); do not use rustdoc intra-doc links (`` [`Invocation`] ``).** Intra-doc links rot silently on renames and cannot target private items; the Stop hook runs `cargo doc -D warnings --document-private-items` on both workspaces and rejects breakage (including stray HTML like `<u8>`). Reference-style file links such as `[SPEC.md ...]: ../../SPEC.md` are not intra-doc links and stay.

6. **Docs and comments state intent in 1–2 sentences; don't explain mechanism.** A doc or comment block answers "what + why" — mechanism is what the code already shows. Drop chained rationale, generic SE narration, and grep-discoverable enumerations; a worked-example pointer (SPEC anchor, file path, named migration) replaces enumerated cases. Applies to RDoc, Rust doc comments, `docs/*.md`, and CLAUDE.md itself.

7. **Route end-to-end coverage through the real mruby guest** (`data/kobako.wasm`). Do not introduce parallel fixture-driven wasm crates; if a behavior cannot be exercised through mruby, prefer a host-side unit test against `Kobako::Outcome` / `Kobako::Transport::Dispatcher` or a hand-rolled minimal wasm module (see `test/fixtures/minimal.wasm`).

8. **`test/` holds gem runtime behavior only.** Build/packaging/lint/static-check wrappers belong in `tasks/*.rake` or top-level scripts. Cross-language integration tests (host↔guest fuzz, ABI invariants) do belong in `test/`.

9. **Commit lock files.** Both `Cargo.lock` (workspace root) and `Gemfile.lock` ship alongside the dependency changes that produced them.

10. **Lock external interfaces before pruning internals.** Settle the outward-facing API first, then prune what sits behind it. Worked example: the `Kobako::Outcome` migration (decode-boundary rename + lift → wire-format simplification → internal absorption), each step kept the previous step's external surface intact.

11. **Test assertion messages are contract statements, not implementation narrative.** Phrase each `assert_*` message as "<input shape> through <public API> must <observable behaviour>"; keep witness rationale in the comment block above the test method. The IO write coverage tests in `test/test_e2e_journeys.rb` are the worked correction example.

## Build Pipeline

The Guest Binary (`data/kobako.wasm`) is gitignored and built via a two-stage rake chain: `beni:build` → `wasm:build`; `rake compile` from a clean clone walks the full chain. Stages A+B (wasi-sdk + mruby vendoring, `libmruby.a`) belong to the beni gem, wired in the Rakefile's `Beni::Tasks` block against `build_config/wasi.rb` (lockfile: `build_config/wasi.rb.lock`); the mrbgem allowlist there is core-extension gems only, with Regexp supplied by the `kobako-regexp` Rust capability gem composed at Stage C. The non-obvious linker choice (rust-lld instead of wasi-sdk's clang, required because `libmruby.a` is not `-fPIC`) is documented inline in `tasks/wasm.rake`. The native ext (`ext/kobako/`) is built separately by `rake compile` via `rb_sys` and links against host-side `wasmtime`, not the guest.

CI (`.github/workflows/main.yml`) runs `bundle exec rake` on Ruby 3.4.7 via `oxidize-rb/actions/setup-ruby-and-rust` — the default task (`compile + test + rubocop + steep`) is the canonical gate.

## Common Commands

| Task | Command |
|------|---------|
| Default CI task (compile + test + rubocop + steep) | `bundle exec rake` |
| Run steep type check only | `bundle exec rake steep` |
| Build native ext (`lib/kobako/kobako.bundle`) | `bundle exec rake compile` |
| Build Guest Binary (full chain) | `bundle exec rake wasm:build` |
| Run all Ruby tests | `bundle exec rake test` |
| Run one Ruby test file | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb` |
| Run one Ruby test by name | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb -n /pattern/` |
| Guest crate host-only tests (wasm32 has no test runner) | `bundle exec rake wasm:test` |
| Guest crate `cargo check` | `bundle exec rake wasm:check` |
| Clean Stage B / Stage C | `rake beni:clean` / `rake wasm:clean` |
| Clean vendor toolchains | `rake beni:vendor:clean` (keeps tarball cache) or `rake beni:vendor:clobber` |
| Interactive REPL with gem loaded | `bin/console` |
| SPEC regression benchmarks (#1..#5, ≤1 MiB payloads) | `bundle exec rake bench` |
| Regression benchmarks + 16 MiB codec sweep | `bundle exec rake bench:full` |
| Concurrent characterization (#6, not gated) | `bundle exec rake bench:concurrent` |
| Memory characterization (#7, not gated) | `bundle exec rake bench:memory` |

## Layering

### Three modules across the wasm boundary

kobako is three source trees that meet at the wasm sandbox boundary. Host and guest are deliberately **wire-symmetric**; `ext/` is the driver that connects them.

```
HOST (process)                                  │  GUEST (wasm32, one Sandbox)
────────────────────────────────────────────── │ ──────────────────────────────────────
lib/  — Ruby gem, the user-facing API           │  wasm/kobako-wasm  — leaf shell (cdylib)
  Sandbox · Catalog · Transport · Outcome        │    KobakoGuest (impl MrbGuest) +
  · Codec · Root        (tier stack below)       │    export_guest! → data/kobako.wasm
       ▲  owns the wire codec                    │  wasm/kobako  — assembled mruby impl
       │  (#encode / .decode, duck-typed)        │    MrbGuest(flows) · Kobako(runtime)
       │                                         │    · KobakoBridge gem · codec convert
       │                                         │  wasm/kobako-io  — IO / Kernel gem
       │                                         │    ::IO · $stdout/$stderr · delegators
       │                                         │  wasm/kobako-core  — contract crate
       │                                         │    Guest trait · abi(primitives) · transport
       │                                         │    · outcome · codec  (mirrors lib/)
       │                                         │       ▲  owns the wire codec
       │                                         │       │  (codec::{Encode,Decode} trait)
       │                                         │  beni / beni-sys  — typed wrapper + FFI
       │                                         │    (crates.io) → libmruby.a (mruby C API)
       ▼                                         │       ▲  kobako-wasm → kobako · kobako-io
ext/  — Rust native ext (magnus + wasmtime)      │       │    · kobako-core; kobako → core ·
  Runtime (Exports, Config) · Invocation ·       │       │    beni; kobako-io → beni
  dispatch · guest_mem · cache · Snapshot        │       │
       └─────────── drives the ABI ─── wasm ─────┼───────┘
                    (alloc / eval / run / take_outcome / dispatch / yield)
```

- **`lib/` ↔ `wasm/kobako-core` are wire-symmetric peers.** Each independently implements the same SPEC wire (MessagePack `Codec` + `Transport` / `Outcome` envelopes) so envelopes round-trip byte-for-byte (the `*_oracle` fuzz checks pin this). Asymmetry that stays: success/failure is a value on the guest (`Outcome` enum) but a return-or-raise on the host (`Outcome.decode` is a module function) — Rust vs Ruby error models.
- **Three publishable guest crates, one shell.** `wasm/kobako-core` is the ABI contract (plain rlib, mruby-free): the `Guest` trait + `export_guest!` macro plus the wire tiers and ABI primitives behind them; it defines no `#[no_mangle]` symbol. `wasm/kobako` is the assembled mruby implementation: the `MrbGuest` harness (required `init_gems` hook + provided flows) and the built-in `KobakoBridge` gem. `wasm/kobako-io` is the IO / Kernel capability gem (`beni::Gem`, kobako-free, pure Rust over wasi-libc). **`wasm/kobako-wasm` is the unpublished cdylib-only shell** composing the three into `data/kobako.wasm`, the same path any third-party guest takes. Published-crate internals follow the beni placeholder rule: compile on every target, fail at runtime in placeholder mode. `kobako-io` is gate-free (pure typed-beni; beni's own placeholder semantics cover it); `kobako` still mirrors the `mruby_linked` cfg in its build.rs for its raw `beni::sys` yield machinery.
- **`ext/` is the host's wasmtime driver, not a wire endpoint.** It drives the ABI exports and shuttles *raw bytes* between Ruby (which owns the codec) and the guest — never decodes envelopes itself. Internal layering mirrors the guest's `abi.rs` (packed-u64, `__kobako_alloc`, linear-memory I/O via `guest_mem`), not the codec.
- **The typed mruby wrapper is the published `beni` crate** (extracted from this repository, developed at [elct9620/beni](https://github.com/elct9620/beni)), consumed directly (`use beni::...`) by `kobako` / `kobako-io` and the shell. Owns `Mrb` / `Ccontext` RAII, the `Value` / `RClass` / `RModule` / `Array` / `Hash` newtypes, the `Module` / `Object` definition traits (`Result<_, Error>`-based), `IntoValue` / `FromValue`, the `Format` + ZST + GAT `mrb_get_args` dispatch, `protect`, `MethodDef` / `method!`, the `Gem` trait, and the typed `mrb_func_t`. Its `beni-sys` FFI layer discovers `libmruby.a` via `MRUBY_LIB_DIR` + `WASI_SDK_PATH` (exported by `rake wasm:build`) and parses the `libmruby.flags.mak` sidecar to keep bindgen's ABI view aligned with the archive.

### `lib/` tier stack

Dependencies point downward — a tier may use the tiers below it, never above. The non-obvious tier is the **root** of dependency-free value objects: they live at `Kobako::*`, *not* under the layer that consumes them, so a lower layer can use them without an upward dependency.

```
Orchestration   Kobako::Sandbox, Kobako::Runtime (+ ext), Kobako::Snapshot
      │
Catalog         Kobako::Catalog::{Namespaces, Snippets, Handles}
      │
Transport ──┐   Kobako::Transport::{Request, Response, Run, Yield, Dispatcher, Yielder}
Outcome ────┤   Kobako::Outcome (decode + Panic)
      │     │
Codec ◄─────┘   Kobako::Codec::{Encoder, Decoder, Factory, Utils}   (byte-level wire)
      │
Root            Kobako::{Handle, Fault, Capture, Usage, Namespace, SandboxOptions},
                Kobako::Snippet::{Source, Binary}, Kobako::Outcome::Panic, Kobako::*Error
                — pure data / invariants, depend on nothing
```

**Placement rule (a `Codec → Transport` cycle bit us once):** a type's namespace follows **dependency direction, not which layer reads it most**. `Kobako::Handle` (0x01) and `Kobako::Fault` (0x02) are consumed almost entirely by Transport, yet sit at the root because `Codec` — below Transport — must register them; nesting them under `Transport` would force `Codec` to depend upward. When unsure, put the type at the **lowest tier that needs it**.

### `ext/` tier stack (Rust native ext, host)

`runtime.rs` (module root) owns the `Kobako::Runtime` magnus class and drives the tiers below it; `Kobako::Snapshot` (`src/snapshot.rs`) is the ext's **root** value object — pure per-invocation carrier (`return_bytes` + capture + usage).

```
Runtime          runtime.rs — Kobako::Runtime class (#from_path / #eval / #run / #usage)
      │
Run mechanics    runtime/{dispatch, guest_mem, trap, capture}
      │            dispatch (__kobako_dispatch) · guest_mem (Caller alloc/write/read)
      │            · trap (error→Kobako::*) · capture (stdout/stderr clip)
      │
Per-Store state  runtime/{invocation, exports, config}
      │            Invocation + StoreCell + KobakoLimiter · Exports (cached handles) · Config (caps)
      │
Process cache    runtime/cache — shared Engine + per-path Module + epoch ticker
```

In `runtime.rs`, reference siblings as bare `dispatch::` / `trap::` (not `super::`, not `use self::dispatch;`).

### Guest crate stack (`wasm/`)

Mirrors `lib/` tier-for-tier — `kobako-core` is the wire-symmetric peer; the `kobako` crate implements the contract over mruby; the cdylib-only `kobako-wasm` shell composes the published crates into `data/kobako.wasm`.

```
kobako-wasm  (unpublished leaf shell, cdylib-only)
Shell           guest — KobakoGuest (impl kobako::MrbGuest; init_gems
                  wires kobako-io) + Guest forwarding + export_guest!
                  emits __kobako_{eval,run,alloc,take_outcome,yield_to_block,abi_version}
────────────────────────────────────────────────────────────────────
kobako  (assembled mruby implementation — publishable rlib)
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
kobako-io  (IO / Kernel capability gem — publishable rlib, kobako-free)
                KobakoIo (impl beni::Gem) — ::IO class, STDOUT/STDERR,
                $stdout/$stderr, private Kernel delegators; pure Rust
                over wasi-libc write(2)
────────────────────────────────────────────────────────────────────
kobako-core  (contract crate — publishable rlib, mruby-free)
Contract        Guest trait (eval / run / yield_to_block) + export_guest! macro
      │
ABI primitives  abi — __kobako_dispatch import · pack/unpack_u64 ·
      │           alloc / take_outcome / write_outcome / write_panic (outcome buffer)
      │         frames — stdin channel reader + Frame 1 preamble parser
Transport ──┐   transport::{Request, Response, Yield, proxy}
Outcome ────┤   outcome::{Outcome, Panic}
      │     │
Codec ◄─────┘   codec::{Encoder, Decoder, Value, Error, Encode, Decode}   (byte-level wire)

(mruby)         beni (typed wrapper) → beni-sys (bindgen FFI) — crates.io;
                consumed by kobako and kobako-io
```

The typed mruby wrapper tiers (`beni` / `beni-sys`) live in the [elct9620/beni](https://github.com/elct9620/beni) repository; see its `CLAUDE.md` for their internal layering. Wrapper-tier changes (new mruby capability, FFI shim, `Format` shape) are beni contributions, released through beni's own gate and consumed here by bumping the `beni` dependency in the guest crates' `Cargo.toml`s.

## Where to Look

Entry points only — siblings (`outcome/panic.rb`, `snippet/{source,binary}.rb`, `transport/request.rb`, etc.) are reachable from there. Notes carry only what reading the entry-point file won't tell you.

| Topic | Entry points | Notes |
|-------|--------------|-------|
| Wire format / codec | host `lib/kobako/codec/`, `lib/kobako/transport/` (envelopes: `request.rb` / `response.rb` / `run.rb` / `yield.rb`); guest `wasm/kobako-core/src/{codec.rs,transport/}` | Envelope shapes: `docs/wire-contract.md`. Byte-level: `docs/wire-codec.md`. Ext-type leaves are root-level: `Kobako::Handle` (0x01), `Kobako::Fault` (0x02). |
| Error taxonomy / outcome | `lib/kobako/errors.rb`, `lib/kobako/outcome.rb` | E-xx anchors in `docs/behavior.md`. |
| Sandbox lifecycle | host `lib/kobako/sandbox.rb`, `ext/kobako/src/runtime.rs`; guest `wasm/kobako/src/flows.rs` | `Kobako::Transport::Run` carries the `#run` host→guest envelope; guest→host dispatch arrives via `Runtime#on_dispatch=` Proc (`lib/kobako/transport/dispatcher.rb`). B-xx in `docs/behavior.md`. |
| Guest IO / `$stdout` / `$stderr` | `wasm/kobako-io/src/{io,kernel}.rs` | Pure-Rust `beni::Gem` (no mrblib / mrbc pipeline, no `beni::sys`); Kernel delegators registered private via `Module::define_private_method`. SPEC B-04. |
| Transport dispatch | host `lib/kobako/transport/dispatcher.rb`; guest `wasm/kobako-core/src/transport/` | Host dispatcher **never raises** — every failure becomes a `Response.err` envelope. |
| Catalog::Handles / capability handles | `lib/kobako/catalog/handles.rb` | B-13..B-21 in `docs/behavior.md`. Owned by Sandbox (B-19), injected into `Kobako::Catalog::Namespaces` so guest→host dispatch and host→guest wire encoding share one allocator. Per-invocation reset is the Sandbox's job. |
| Service registration | `lib/kobako/catalog/namespaces.rb`, `lib/kobako/namespace.rb` | Per-Sandbox `Catalog::Namespaces` owns the `Kobako::Namespace` registry; bound objects live at `"<Namespace>::<Member>"` (e.g., `"MyService::KV"`). |
| ABI surface (host ↔ guest exports) | contract `wasm/kobako-core/src/guest.rs` (`Guest` + `export_guest!`); entry bodies `wasm/kobako/src/flows.rs` ↔ `ext/kobako/src/runtime.rs` | — |
| E2E coverage | `test/test_e2e_journeys.rb` (`#eval`), `test/test_sandbox_run.rb` (`#run`) | Both drive real `data/kobako.wasm`. Wrapper-tier (`test/test_wasm_wrapper.rb`) covers only `from_path`. |
| mruby typed wrapper / FFI | `beni` + `beni-sys` crates ([elct9620/beni](https://github.com/elct9620/beni)) | Consumed directly by the guest crates (`use beni::...`; raw FFI via the `beni::sys` re-export). Wrapper changes are beni contributions, pulled in by a dependency bump. |
| RBS signatures | `sig/kobako/` (mirrors `lib/kobako/` 1:1) | Three sources stack: `sig/_external/` (hand-rolled), `rbs_collection.{yaml,lock.yaml}` (gem), `library "<name>"` in `Steepfile` (stdlib — reach for first). PostToolUse steep hook blocks Ruby edits without matching `.rbs`. |
| Regression benchmarks | `tasks/benchmark.rake`, `benchmark/` | #1..#5 gated (+10% regression blocks release); #6/#7 characterization, not gated. Results: `benchmark/results/<date>-<short-sha>.json`. |
| Build / toolchain | Rakefile (`Beni::Tasks` block), `build_config/wasi.rb`, `tasks/wasm.rake` | Stages A+B live in the beni gem (`rake beni:build`); kobako keeps only the build config and Stage C. |

`test/test_helper.rb` rescues `LoadError` when `lib/kobako/kobako.bundle` is missing and stubs `Kobako::Error`, so the suite still loads on a clean checkout; individual tests `skip` themselves when the native ext is absent.
