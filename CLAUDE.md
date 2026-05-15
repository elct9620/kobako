# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

kobako is a Ruby gem that provides an in-process Wasm sandbox for running untrusted mruby scripts. The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing the mruby interpreter; host and guest communicate over a MessagePack-based RPC wire defined in `SPEC.md`.

## Principles

Apply these in order — earlier principles override later ones on conflict.

1. **SPEC.md is the source of truth.** Behavior contracts (Wire Codec, error taxonomy, Sandbox lifecycle, HandleTable rules) live in `SPEC.md`. Cite anchors (B-xx / E-xx) from committed code as an RDoc link resolved from the file's location — e.g. `{SPEC.md B-04}[link:../../SPEC.md]` from `lib/kobako/*.rb`. When SPEC is silent, extend `SPEC.md` first, then cite the new anchor.

2. **One thing per file; keep files small.** When a class grows, split it into a façade plus per-responsibility files in a sibling directory. `Registry` and `Wire::Envelope` follow this pattern (`<name>.rb` façade + `<name>/` directory of focused files). Prefer adding a new file over expanding an existing one.

   **Types nest under a Module, not a Class.** A stateful Class is per-instance and should not double as the namespace for sibling types. Place new types at the top level (`Kobako::Capture`, `Kobako::Outcome`) or under a Module (`Kobako::Wire::Envelope::Panic`). The `Registry::HandleTable` family — Classes nested under a Class — is residue from earlier development; do not extend the pattern.

3. **Keep it simple. Don't pre-abstract.** Model exactly what SPEC requires — no speculative interfaces, parallel hierarchies, or defensive layers. Three similar lines beats a premature abstraction; a one-shot operation does not need a helper. Avoid feature flags and backwards-compatibility shims when the code can just change.

4. **Follow language community conventions via tooling.** Ruby: Rubocop (auto-applied on `.rb` Edit/Write via PostToolUse hook) plus Steep (`bundle exec steep check` runs on every `.rb` / `.rbs` Edit/Write — failures block the edit). Rust: `cargo fmt` and `cargo clippy` (both auto-applied on `.rs` Edit/Write via PostToolUse hook; clippy runs with `-D warnings`, and uses `--target wasm32-wasip1` for `wasm/*` manifests when the cross-toolchain is provisioned). When a cop or lint fires, **shrink the code to fit the tool** — don't widen `.rubocop.yml` exclusions or add `#[allow]` / `# steep:ignore`. Existing exclusions on `lib/kobako/wire/**`, `tasks/*.rake`, and `test/**` are anchored to specific SPEC-to-code mappings; add new ones only with an inline comment naming the mapping.

   **Tool-vs-tool conflicts are the one justified widening.** When a Rubocop cop and Steep / RBS upstream disagree on the same code shape — currently `Style/DataInheritance` rejects `class X < Data.define(...)` while ruby/rbs ([`docs/data_and_struct.md`](https://github.com/ruby/rbs/blob/master/docs/data_and_struct.md)) documents the subclass form as the Steep-friendly pattern — prefer the type-system guidance and disable the cop at the `.rubocop.yml` level with a comment citing the upstream source. The six `Data.define` types in `lib/kobako/wire/**` still pay the `# steep:ignore` tax because that migration has not yet happened; the transitional state is known, the target is the subclass form.

5. **Document Ruby in RDoc prose.** No tool enforces this — match the existing style. Class doc explains purpose, ownership, and SPEC invariants; method doc describes parameters, return value, and raised exceptions in prose paragraphs. Wrap identifiers in `+code+`. Cite SPEC as `{SPEC.md B-XX}[link:<relative path>]` in plain text (no glyphs like the section sign). Do not use YARD tags (`@param` / `@return` / `@raise`); migrate them when touching nearby code.

   ```ruby
   # Host-side mapping from opaque integer Handle IDs to Ruby objects.
   # One table is owned per Kobako::Registry instance. See
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

6. **Route end-to-end coverage through the real mruby guest** (`data/kobako.wasm`). Do not introduce parallel fixture-driven wasm crates; if a behavior cannot be exercised through mruby, prefer a host-side unit test against `Kobako::Outcome` / `Registry::Dispatcher` or a hand-rolled minimal wasm module (see `test/fixtures/minimal.wasm`).

7. **`test/` holds gem runtime behavior only.** Build/packaging/lint/static-check wrappers belong in `tasks/*.rake` or top-level scripts. Cross-language integration tests (host↔guest fuzz, ABI invariants) do belong in `test/`.

8. **Commit lock files.** Both `Cargo.lock` (workspace root) and `Gemfile.lock` ship alongside the dependency changes that produced them.

9. **Lock external interfaces before pruning internals.** When a module has accumulated delegate / pass-through layers, settle the outward-facing API first, then prune what sits behind it. A stable outer interface is a stable target for inner cleanup; reshuffling internals against a moving target compounds churn. The `Kobako::Outcome` migration is the worked example — the host-facing decode boundary landed first (rename + lift to top level), then the wire-format change dropped the 1-elem array wrap, then the internal Wire residue (Result / Panic / Outcome envelope types and their encoders) got absorbed under `Kobako::Outcome`. Each step kept the previous step's external surface intact.

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

When changing behavior, start at the listed files and follow the SPEC anchors they cite. Each entry names only the **load-bearing** files — incidental helpers are reachable from there.

- **Wire format / codec** — `lib/kobako/wire/` (host) + `wasm/kobako-wasm/src/codec/`, `wasm/kobako-wasm/src/envelope.rs` (guest). SPEC anchors: B-01..B-14.
- **Error taxonomy / outcome attribution** — `lib/kobako/errors.rb` + `lib/kobako/outcome.rb`. SPEC anchors: E-xx.
- **Sandbox lifecycle / per-run flow** — `lib/kobako/sandbox.rb` + `lib/kobako/capture.rb` + `ext/kobako/src/wasm.rs` (host orchestration) + `wasm/kobako-wasm/src/abi.rs` (guest entry — `__kobako_run` / `__kobako_alloc` / `__kobako_take_outcome` exports).
- **RPC dispatch** — `lib/kobako/registry/dispatcher.rb` (host; **never raises** — every failure becomes a `Response.err` envelope) + `wasm/kobako-wasm/src/rpc_client.rs` (guest).
- **HandleTable / capability handles** — `lib/kobako/registry/handle_table.rb` + `lib/kobako/wire/handle.rb`.
- **Service registration** — `lib/kobako/registry.rb` + `lib/kobako/registry/service_group.rb`.
- **ABI surface (host ↔ guest exports)** — `wasm/kobako-wasm/src/abi.rs` + matching `ext/kobako/src/wasm.rs` callers.
- **E2E coverage / real guest binary** — `test/test_e2e_journeys.rb` drives `Sandbox#run` against `data/kobako.wasm`; it is the load-bearing coverage for Wire ABI export presence and the host↔guest round-trip. Wrapper-tier tests (`test/test_wasm_wrapper.rb`) cover only the `from_path` pipeline and intentionally do not duplicate this — a missing export fails the first E2E journey, so adding a wrapper-tier smoke test is redundant.
- **Build / toolchain** — `tasks/{vendor,mruby,wasm}.rake`.
- **RBS type signatures** — `sig/kobako/` mirrors `lib/kobako/` one-to-one (façade `.rbs` + sibling directory for split classes). `sig/_external/` holds hand-written stubs for third-party gems that lack public RBS; `rbs_collection.yaml` + `rbs_collection.lock.yaml` pin the rest from gem_rbs_collection. `Steepfile` checks `lib` against `sig`. When adding a Ruby class, write its `.rbs` alongside — the PostToolUse steep hook will block the edit otherwise.
- **Regression benchmarks** — `tasks/benchmark.rake` + `benchmark/{cold_start,rpc_roundtrip,codec,mruby_eval,handle_table}.rb` are the five SPEC-mandated benchmarks (#1..#5); a +10% regression on any one blocks release. Characterization scripts (`benchmark/concurrent/`, `benchmark/memory.rb`) are #6/#7 and are not in the release gate. Results land in `benchmark/results/<date>-<short-sha>.json`. See `benchmark/README.md` for per-suite scope and known caveats (guest String size cap, GVL bounds, allocator retention).

`test/test_helper.rb` `rescue`s the `LoadError` when `lib/kobako/kobako.bundle` is missing and stubs `Kobako::Error`, so the suite still loads on a clean checkout; individual tests check `defined?(Kobako::Wasm::Instance)` and `skip` themselves when the native ext is absent.
