# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

kobako is a Ruby gem that provides an in-process Wasm sandbox for running untrusted mruby scripts. The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing the mruby interpreter; host and guest communicate over a MessagePack-based RPC wire defined in `SPEC.md`.

## Principles

Apply these in order — earlier principles override later ones on conflict.

1. **SPEC.md is the source of truth.** Behavior contracts live in `SPEC.md` or in the `docs/<topic>.md` it indexes. Cite anchors as `{SPEC.md B-04}[link:../../SPEC.md]` from `lib/kobako/*.rb`; switch to `{docs/<topic>.md B-04}[link:../../docs/<topic>.md]` once the anchor moves. B-xx / E-xx numbers are append-only across the corpus; existing anchors are never renumbered. When SPEC is silent, extend it (or the relevant `docs/<topic>.md`) first, then cite the new anchor.

2. **One thing per file; keep files small.** When a module grows, split it into a façade plus per-responsibility files in a sibling directory — `Kobako::RPC` and `Kobako::Snippet` are the worked examples. Prefer adding a new file over expanding an existing one.

   **Types nest under a Module, not a Class.** A stateful Class is per-instance and should not double as the namespace for sibling types. Place new types at the top level (`Kobako::Capture`, `Kobako::Invocation`) or under a Module (`Kobako::RPC::Envelope::Request`, `Kobako::Outcome::Panic`). Do not introduce Classes nested under a Class for type-grouping purposes.

3. **Keep it simple. Don't pre-abstract.** Model exactly what SPEC requires — no speculative interfaces, parallel hierarchies, or defensive layers. Three similar lines beats a premature abstraction; a one-shot operation does not need a helper. Avoid feature flags and backwards-compatibility shims when the code can just change.

4. **Follow language community conventions via tooling.** Ruby: Rubocop + Steep. Rust: `cargo fmt` + `cargo clippy -D warnings` (clippy also under `--target wasm32-wasip1`). All four run on every Edit/Write via PostToolUse hooks; failures block the edit. When a cop or lint fires, **shrink the code to fit the tool** — don't widen `.rubocop.yml` exclusions or add `#[allow]` / `# steep:ignore`. Existing exclusions are anchored to specific SPEC-to-code mappings; add new ones only with an inline comment naming the mapping.

   **Tool-vs-tool conflicts are the one justified widening.** When Rubocop and Steep / RBS upstream disagree on the same code shape, prefer the type-system guidance and disable the cop at the `.rubocop.yml` level with a comment citing the upstream source. `Style/DataInheritance` is disabled on that basis (ruby/rbs [`docs/data_and_struct.md`](https://github.com/ruby/rbs/blob/master/docs/data_and_struct.md) documents the `class X < Data.define(...)` subclass form as the Steep-friendly pattern). `Kobako::Outcome::Panic`, `Kobako::Invocation`, `Kobako::SandboxOptions`, and `Kobako::Snippet::{Source,Binary}` are the migrated examples; the four `Data.define` types still on the assignment form in `lib/kobako/rpc/**` are transitional debt — migrate when touching nearby code, don't write new types in the assignment form.

5. **Document Ruby in RDoc prose.** No tool enforces this — match the existing style. Wrap identifiers in `+code+`. Cite SPEC as `{SPEC.md B-XX}[link:<relative path>]` in plain text. Do not use YARD tags (`@param` / `@return` / `@raise`); migrate them when touching nearby code.

   ```ruby
   # Host-side mapping from opaque integer Handle IDs to Ruby objects.
   # One table is owned per Kobako::RPC::Server instance. See
   # {SPEC.md B-15}[link:../../../SPEC.md].
   #
   #   - {SPEC.md B-15}[link:../../../SPEC.md] — IDs are monotonically
   #     allocated per +#run+; ID 0 is the invalid sentinel.
   #   - {SPEC.md B-21}[link:../../../SPEC.md] — Cap is +MAX_ID+; allocation
   #     beyond raises immediately, no wrap, no reuse.
   class HandleTable
     # Bind +object+ in the table and return its newly-allocated Handle ID.
     # +object+ is any host-side Ruby object to bind. Returns a freshly-
     # allocated Handle ID in +[1, MAX_ID]+. Raises
     # +Kobako::HandleTableExhausted+ if the next ID would exceed the cap.
     def alloc(object)
       # ...
     end
   end
   ```

6. **Route end-to-end coverage through the real mruby guest** (`data/kobako.wasm`). Do not introduce parallel fixture-driven wasm crates; if a behavior cannot be exercised through mruby, prefer a host-side unit test against `Kobako::Outcome` / `Kobako::RPC::Dispatcher` or a hand-rolled minimal wasm module (see `test/fixtures/minimal.wasm`).

7. **`test/` holds gem runtime behavior only.** Build/packaging/lint/static-check wrappers belong in `tasks/*.rake` or top-level scripts. Cross-language integration tests (host↔guest fuzz, ABI invariants) do belong in `test/`.

8. **Commit lock files.** Both `Cargo.lock` (workspace root) and `Gemfile.lock` ship alongside the dependency changes that produced them.

9. **Lock external interfaces before pruning internals.** When a module has accumulated delegate / pass-through layers, settle the outward-facing API first, then prune what sits behind it. The `Kobako::Outcome` migration is the worked example: decode-boundary rename + lift first, then wire-format simplification, then internal absorption — each step kept the previous step's external surface intact.

10. **Test assertion messages are contract statements, not implementation narrative.** Phrase each `assert_*` message as "<input shape> through <public API> must <observable behaviour>"; keep witness rationale (why this boundary value, why this branch matters) in the comment block above the test method. The IO write coverage tests in `test/test_e2e_journeys.rb` are the worked correction example.

## Build Pipeline

The Guest Binary (`data/kobako.wasm`) is gitignored and built via a three-stage rake chain: `vendor:setup` → `mruby:build` → `wasm:build`. `rake compile` from a clean clone walks the full chain. The non-obvious linker choice (rust-lld instead of wasi-sdk's clang, required because `libmruby.a` is not `-fPIC`) is documented inline in `tasks/wasm.rake` `cargo_build_env`. The native ext (`ext/kobako/`) is built separately by `rake compile` via `rb_sys` and links against host-side `wasmtime`, not the guest.

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
| Clean Stage B / Stage C | `rake mruby:clean` / `rake wasm:clean` |
| Clean vendor toolchains | `rake vendor:clean` (keeps tarball cache) or `rake vendor:clobber` |
| Interactive REPL with gem loaded | `bin/console` |
| SPEC regression benchmarks (#1..#5, ≤1 MiB payloads) | `bundle exec rake bench` |
| Regression benchmarks + 16 MiB codec sweep | `bundle exec rake bench:full` |
| Concurrent characterization (#6, not gated) | `bundle exec rake bench:concurrent` |
| Memory characterization (#7, not gated) | `bundle exec rake bench:memory` |

## Where to Look

Entry points only — siblings (`outcome/panic.rb`, `snippet/{source,binary}.rb`, `rpc/handle.rb`, etc.) are reachable from there. The Notes column carries only what reading the entry-point file won't tell you.

| Topic | Entry points | Notes |
|-------|--------------|-------|
| Wire format / codec | host `lib/kobako/codec/`, `lib/kobako/rpc/envelope.rb`; guest `wasm/kobako-wasm/src/{codec,rpc}/` | Envelope shapes: `docs/wire-contract.md`. Byte-level: `docs/wire-codec.md`. |
| Error taxonomy / outcome | `lib/kobako/errors.rb`, `lib/kobako/outcome.rb` | E-xx anchors in `docs/behavior.md`. |
| Sandbox lifecycle | host `lib/kobako/sandbox.rb`, `ext/kobako/src/wasm.rs`; guest `wasm/kobako-wasm/src/abi.rs` | `Kobako::Invocation` sits top-level (host→guest dispatch), not under `RPC::` (guest→host channel). B-xx in `docs/behavior.md`. |
| Guest IO / `$stdout` / `$stderr` | `wasm/kobako-wasm/src/kobako/io.rs`, `wasm/kobako-wasm/mrblib/{io,kernel}.rb` | mrblib is precompiled to RITE bytecode by `build.rs` and embedded via `src/kobako/bytecode.rs`. SPEC B-04. |
| RPC dispatch | host `lib/kobako/rpc/dispatcher.rb`; guest `wasm/kobako-wasm/src/rpc/` | Host dispatcher **never raises** — every failure becomes a `Response.err` envelope. |
| HandleTable / capability handles | `lib/kobako/rpc/handle_table.rb` | B-13..B-21 in `docs/behavior.md`. |
| Service registration | `lib/kobako/rpc/server.rb`, `lib/kobako/rpc/namespace.rb` | Per-Sandbox Server owns the Namespace registry + HandleTable; bound objects live one level deep at `"Namespace::Member"`. |
| ABI surface (host ↔ guest exports) | `wasm/kobako-wasm/src/abi.rs` ↔ `ext/kobako/src/wasm.rs` | — |
| E2E coverage | `test/test_e2e_journeys.rb` (`#eval`), `test/test_sandbox_run.rb` (`#run`) | Both drive real `data/kobako.wasm`. Wrapper-tier (`test/test_wasm_wrapper.rb`) covers only `from_path` and deliberately does not duplicate ABI-export checks. |
| mruby C API FFI | `wasm/kobako-mruby-sys/` (`wrapper.h`, `build.rs`, `src/{state,value,class,ccontext,array,hash}.rs`) | bindgen scoped to this crate (libclang stays sys-only); `wrap_static_fns` emits a single C trampoline — no hand-written `.c` shims. Consumed by `kobako-wasm` via the `crate::mruby` façade. |
| RBS signatures | `sig/kobako/` (mirrors `lib/kobako/` 1:1) | Three sources stack: `sig/_external/` (hand-rolled), `rbs_collection.{yaml,lock.yaml}` (gem), and `library "<name>"` in `Steepfile` (stdlib — reach for this first). PostToolUse steep hook blocks Ruby edits without matching `.rbs`. |
| Regression benchmarks | `tasks/benchmark.rake`, `benchmark/` | #1..#5 are gated (+10% regression blocks release); #6/#7 are characterization, not gated. Results: `benchmark/results/<date>-<short-sha>.json`. Scope + caveats in `benchmark/README.md`. |
| Build / toolchain | `tasks/{vendor,mruby,wasm}.rake` | — |

`test/test_helper.rb` rescues `LoadError` when `lib/kobako/kobako.bundle` is missing and stubs `Kobako::Error`, so the suite still loads on a clean checkout; individual tests `skip` themselves when the native ext is absent.
