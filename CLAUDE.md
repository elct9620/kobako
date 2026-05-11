# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

kobako is a Ruby gem that provides an in-process Wasm sandbox for running untrusted mruby scripts. The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing the mruby interpreter; host and guest communicate over a MessagePack-based RPC wire (`SPEC.md` is the authoritative contract — anything in this file is a navigational aid, not the spec).

`SPEC.md` is the single source of truth for behavior — Wire Codec, error taxonomy, Sandbox lifecycle, HandleTable rules (B-xx / E-xx anchors are referenced throughout the codebase).

## Build Pipeline (three stages)

The Guest Binary (`data/kobako.wasm`) is gitignored and produced by a three-stage chain. Stage C depends on Stages A and B, so a one-shot `rake compile` from a clean clone walks the full chain:

- **Stage A — `rake vendor:setup`** — downloads/unpacks pinned `wasi-sdk` and `mruby` tarballs into `vendor/` (sentinel-based idempotency; `vendor/` is gitignored). Honors `KOBAKO_VENDOR_DIR` and `KOBAKO_VENDOR_BASE_URL` for test fixtures.
- **Stage B — `rake mruby:build`** — cross-compiles `libmruby.a` for `wasm32-wasip1` via mruby's vendored `minirake` driven by `build_config/wasi.rb`. Output: `vendor/mruby/build/wasi/lib/libmruby.a`. Skips when sentinel exists.
- **Stage C — `rake wasm:guest`** — `cargo build --release --target wasm32-wasip1` on the `wasm/kobako-wasm/` crate, linking libmruby.a, then copies the artifact to `data/kobako.wasm`. Uses **rust-lld** (not wasi-sdk's clang) because libmruby.a and Rust's wasi prebuilts are not `-fPIC` — see the long comment in `tasks/wasm.rake` `cargo_build_env`.

The native ext (`ext/kobako/`) is built separately by `rake compile` via `rb_sys` and links against host-side `wasmtime` (not the guest).

## Common Commands

| Task | Command |
|------|---------|
| Default CI task (compile + test + rubocop) | `bundle exec rake` |
| Build native ext (`lib/kobako/kobako.bundle`) | `bundle exec rake compile` |
| Build Guest Binary (full A→B→C chain) | `bundle exec rake wasm:guest` |
| Build host-side E2E test fixture | `bundle exec rake fixtures:test_guest` |
| Run all Ruby tests | `bundle exec rake test` |
| Run one Ruby test file | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb` |
| Run one Ruby test by name | `bundle exec ruby -Ilib -Itest test/test_sandbox.rb -n /pattern/` |
| Rubocop (auto-applied via PostToolUse hook on `.rb` edits) | `bundle exec rubocop -A` |
| Guest crate host-only tests (wasm32 has no test runner) | `bundle exec rake wasm:test` |
| Guest crate `cargo check` | `bundle exec rake wasm:check` |
| Clean Stage B / Stage C | `rake mruby:clean` / `rake wasm:guest:clean` |
| Clean vendor toolchains | `rake vendor:clean` (keeps tarball cache) or `rake vendor:clobber` |
| Interactive REPL with gem loaded | `bin/console` |

`test/test_helper.rb` degrades gracefully when `lib/kobako/kobako.bundle` is missing — tests that need the native ext call `skip` so the suite still loads on a clean checkout.

## Architecture

### Three-process-but-one model

There are three independently-compiled artifacts that all participate in one `Sandbox#run`:

1. **Host Ruby (`lib/`)** — public API (`Kobako::Sandbox`, `Kobako::Registry`, `Kobako::Wire::*`). All loaded by `lib/kobako.rb`.
2. **Host native ext (`ext/kobako/` → `lib/kobako/kobako.bundle`)** — Rust + magnus + `wasmtime` + `wasmtime-wasi`. Surfaces `Kobako::Wasm::{Engine, Module, Store, Instance}` plus error classes from Rust at load time. Source in `ext/kobako/src/{lib.rs,wasm.rs}`.
3. **Guest binary (`wasm/kobako-wasm/` → `data/kobako.wasm`)** — Rust + `rmp` + linked `libmruby.a`. Compiled for `wasm32-wasip1`. Exports the SPEC ABI (`__kobako_run`, `__kobako_alloc`, `__kobako_take_outcome`, …) and embeds the mruby interpreter and RPC client.

### Per-run flow (`Kobako::Sandbox#run`)

1. `Registry#seal!` — locks the Service group/member definitions.
2. `Registry#reset_handles!` + clear stdout/stderr `OutputBuffer`s.
3. Build the **two-frame stdin protocol**: Frame 1 = msgpack-packed preamble (Service Group registry snapshot), Frame 2 = mruby source UTF-8 bytes. Each prefixed by a 4-byte big-endian u32 length.
4. Native ext invokes guest `__kobako_run`; WASI stdout/stderr pipes are drained into bounded `OutputBuffer`s (1 MiB cap each by default, truncate-with-`[truncated]` marker — SPEC §B-04).
5. Read OUTCOME_BUFFER bytes; first byte is the tag (`0x01` Result, `0x02` Panic), rest is a msgpack envelope. `Sandbox#decode_outcome` implements the three-layer attribution:
   - Tag `0x01` → return decoded value (or `SandboxError` if envelope decode fails — E-09)
   - Tag `0x02`, `origin="service"` → `ServiceError` (or `ServiceError::Disconnected` for the E-14 sentinel)
   - Tag `0x02`, `origin="sandbox"`/missing → `SandboxError` (E-04..E-07)
   - Unknown tag / zero len → `TrapError` (E-02 / E-03)

### Error taxonomy (`lib/kobako/errors.rb`)

Three top-level branches, all under `Kobako::Error`:

- `TrapError` — Wasm engine crash or wire-violation fallback (corrupted guest state)
- `SandboxError` — guest ran but failed (mruby error, protocol fault, wire decode failure on a valid tag, HandleTable exhaustion)
  - `HandleTableError < SandboxError` — unknown id lookup
  - `HandleTableExhausted < HandleTableError` — id cap hit (B-21)
- `ServiceError` — unrescued Service capability failure inside the script
  - `ServiceError::Disconnected < ServiceError` — Handle resolved to `:disconnected` sentinel (E-14, ABA protection)

### Wire codec (`lib/kobako/wire/`)

MessagePack with two registered ext types: `0x01` Capability Handle, `0x02` Exception envelope. The host side is built on the `msgpack` gem + a `Factory` (`lib/kobako/wire/factory.rb`); the guest side uses the `rmp` crate. Envelope kinds (Request/Response/Result/Panic) and the outer Outcome wrapper live in `lib/kobako/wire/envelope.rb`. **`Wire::Envelope` is the only place where binary framing rules live** — Encoder/Decoder primitives stay byte-only.

### Registry / HandleTable

The registry internals are split across four files:

- `lib/kobako/registry.rb` — façade: the public `Registry` class that owns and delegates to the internal components.
- `lib/kobako/registry/service_group.rb` — `ServiceGroup` definition and member registration.
- `lib/kobako/registry/handle_table.rb` — `HandleTable`: opaque integer Handle ID ↔ Ruby object mapping.
- `lib/kobako/registry/dispatcher.rb` — `Registry#dispatch(request_bytes)`: host-side RPC entry point called from the native ext during `__kobako_rpc_call`; it **never raises** — every failure is reified as a `Response.err` envelope with one of these `type`s: `"undefined"`, `"disconnected"`, `"argument"`, `"runtime"`.

Service methods whose return value is not wire-representable are routed through `HandleTable#alloc` and the guest receives a `Wire::Handle` (B-14). HandleTable IDs are scoped to one `#run` (B-15) and capped at `0x7fff_ffff` (B-21).

### Cargo workspace layout

The repo-root `Cargo.toml` is a workspace containing only `ext/kobako` (the host extension); `wasm/` is **excluded** so it can be its own workspace root. This keeps host-only `wasmtime` out of the wasm32-wasip1 dependency graph, and lets the guest crate (`wasm/kobako-wasm`) compile against `rmp` + `libmruby.a` without dragging in host dependencies. `wasm/test-guest/` is a sibling guest workspace producing a stub binary for native-ext E2E tests.

## Conventions

- **Cite SPEC.md.** Reference behavior contracts in committed code via SPEC.md anchors (B-xx / E-xx). When the spec is silent, extend SPEC.md first, then cite the new anchor. Render the citation as an RDoc link — `{SPEC.md §B-04}[link:../../SPEC.md]` — with the relative path resolved from the current file's location (e.g. `../../SPEC.md` from `lib/kobako/*.rb`).
- **Commit lock files.** Both `Cargo.lock` (workspace root) and `Gemfile.lock` are committed alongside dependency changes.
- **Ruby comments use RDoc format.** Describe parameters, return values, and raised exceptions in prose paragraphs; wrap identifiers in `+code+` and use `:call-seq:` for non-obvious call signatures. Prefer plain prose over tag-style annotations (`@param` / `@return` / `@raise` belong to YARD and should be migrated to RDoc when touching nearby code).
- **`test/` hosts Minitest specs of gem runtime behavior.** Keep build/packaging/lint/static-check wrappers under `tasks/*.rake` or top-level scripts. Cross-language integration tests (host↔guest fuzz, ABI invariants) live in `test/`.
- **Route end-to-end coverage through the real mruby guest** (`data/kobako.wasm`). Reserve `wasm/test-guest/` for native-ext exercises that cannot involve mruby.
- **Rubocop runs automatically** on every `Edit`/`Write` to a `.rb` file via the `.claude/settings.json` PostToolUse hook (`bundle exec rubocop -A`). Exit code 2 means autocorrect did not converge — fix the offense and retry.
- **Keep `.rubocop.yml` exclusions narrow.** Existing exclusions on `lib/kobako/wire/**`, `tasks/*.rake`, and `test/**` exist because the SPEC-to-code mapping (codec dispatch, rake DSL, fuzz fixtures) outweighs the cop's heuristic. Add new exclusions only with an inline comment explaining the mapping that motivates them.
