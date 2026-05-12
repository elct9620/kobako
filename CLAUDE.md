# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

kobako is a Ruby gem that provides an in-process Wasm sandbox for running untrusted mruby scripts. The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing the mruby interpreter; host and guest communicate over a MessagePack-based RPC wire defined in `SPEC.md`.

## Principles

Apply these in order — earlier principles override later ones on conflict.

1. **SPEC.md is the source of truth.** Behavior contracts (Wire Codec, error taxonomy, Sandbox lifecycle, HandleTable rules) live in `SPEC.md`. Cite anchors (B-xx / E-xx) from committed code as an RDoc link resolved from the file's location — e.g. `{SPEC.md B-04}[link:../../SPEC.md]` from `lib/kobako/*.rb`. When SPEC is silent, extend `SPEC.md` first, then cite the new anchor.

2. **One thing per file; keep files small.** When a class grows, split it into a façade plus per-responsibility files in a sibling directory. `Registry`, `Wire::Envelope`, and `Sandbox` all follow this pattern (`<name>.rb` façade + `<name>/` directory of focused files). Prefer adding a new file over expanding an existing one.

3. **Keep it simple. Don't pre-abstract.** Model exactly what SPEC requires — no speculative interfaces, parallel hierarchies, or defensive layers. Three similar lines beats a premature abstraction; a one-shot operation does not need a helper. Avoid feature flags and backwards-compatibility shims when the code can just change.

4. **Follow language community conventions via tooling.** Ruby: Rubocop (auto-applied on `.rb` Edit/Write via PostToolUse hook). Rust: `cargo fmt` and `cargo clippy`. When a cop or lint fires, **shrink the code to fit the tool** — don't widen `.rubocop.yml` exclusions or add `#[allow]`. Existing exclusions on `lib/kobako/wire/**`, `tasks/*.rake`, and `test/**` are anchored to specific SPEC-to-code mappings; add new ones only with an inline comment naming the mapping.

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

5. **Route end-to-end coverage through the real mruby guest** (`data/kobako.wasm`). Reserve `wasm/test-guest/` for native-ext exercises that cannot involve mruby; do not add new test-guest dialects.

6. **`test/` holds gem runtime behavior only.** Build/packaging/lint/static-check wrappers belong in `tasks/*.rake` or top-level scripts. Cross-language integration tests (host↔guest fuzz, ABI invariants) do belong in `test/`.

7. **Commit lock files.** Both `Cargo.lock` (workspace root) and `Gemfile.lock` ship alongside the dependency changes that produced them.

## Build Pipeline

The Guest Binary (`data/kobako.wasm`) is gitignored and built via a three-stage rake chain: `vendor:setup` → `mruby:build` → `wasm:guest`. `rake compile` from a clean clone walks the full chain. The non-obvious linker choice (rust-lld instead of wasi-sdk's clang, required because `libmruby.a` is not `-fPIC`) is documented inline in `tasks/wasm.rake` `cargo_build_env`. The native ext (`ext/kobako/`) is built separately by `rake compile` via `rb_sys` and links against host-side `wasmtime`, not the guest.

## Common Commands

| Task | Command |
|------|---------|
| Default CI task (compile + test + rubocop) | `bundle exec rake` |
| Build native ext (`lib/kobako/kobako.bundle`) | `bundle exec rake compile` |
| Build Guest Binary (full chain) | `bundle exec rake wasm:guest` |
| Build host-side E2E test fixture | `bundle exec rake fixtures:test_guest` |
| Run all Ruby tests | `bundle exec rake test` |
| Run one Ruby test file | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb` |
| Run one Ruby test by name | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb -n /pattern/` |
| Guest crate host-only tests (wasm32 has no test runner) | `bundle exec rake wasm:test` |
| Guest crate `cargo check` | `bundle exec rake wasm:check` |
| Clean Stage B / Stage C | `rake mruby:clean` / `rake wasm:guest:clean` |
| Clean vendor toolchains | `rake vendor:clean` (keeps tarball cache) or `rake vendor:clobber` |
| Interactive REPL with gem loaded | `bin/console` |

## Where to Look

When changing behavior, start at the listed files and follow the SPEC anchors they cite. Each entry names only the **load-bearing** files — incidental helpers are reachable from there.

- **Wire format / codec** — `lib/kobako/wire/` (host) + `wasm/kobako-wasm/src/codec/`, `wasm/kobako-wasm/src/envelope.rs` (guest). SPEC anchors: B-01..B-14.
- **Error taxonomy / outcome attribution** — `lib/kobako/errors.rb` + `lib/kobako/sandbox/outcome_decoder.rb`. SPEC anchors: E-xx.
- **Sandbox lifecycle / per-run flow** — `lib/kobako/sandbox.rb` (façade) + `lib/kobako/sandbox/*` + `ext/kobako/src/wasm.rs` (host orchestration) + `wasm/kobako-wasm/src/boot.rs` (guest entry).
- **RPC dispatch** — `lib/kobako/registry/dispatcher.rb` (host; **never raises** — every failure becomes a `Response.err` envelope) + `wasm/kobako-wasm/src/rpc_client.rs` (guest).
- **HandleTable / capability handles** — `lib/kobako/registry/handle_table.rb` + `lib/kobako/wire/handle.rb`.
- **Service registration** — `lib/kobako/registry.rb` + `lib/kobako/registry/service_group.rb`.
- **ABI surface (host ↔ guest exports)** — `wasm/kobako-wasm/src/abi.rs` + matching `ext/kobako/src/wasm.rs` callers.
- **Build / toolchain** — `tasks/{vendor,mruby,wasm}.rake`.

`test/test_helper.rb` `skip`s native-ext-dependent tests when `lib/kobako/kobako.bundle` is missing, so the suite still loads on a clean checkout.
