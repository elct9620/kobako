# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

kobako is a Ruby gem that provides an in-process Wasm sandbox for running untrusted mruby scripts. The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing the mruby interpreter; host and guest communicate over a MessagePack-based Transport wire defined in `SPEC.md`.

## Principles

Apply these in order — earlier principles override later ones on conflict.

1. **SPEC.md is the source of truth.** Behavior contracts live in `SPEC.md` or in the `docs/<topic>.md` it indexes. Cite anchors as `{SPEC.md B-04}[link:../../SPEC.md]` from `lib/kobako/*.rb`; switch to `{docs/<topic>.md B-04}[link:../../docs/<topic>.md]` once the anchor moves. B-xx / E-xx numbers are append-only across the corpus; existing anchors are never renumbered. When SPEC is silent, extend it (or the relevant `docs/<topic>.md`) first, then cite the new anchor.

2. **One thing per file; keep files small.** When a module grows, split it into a façade plus per-responsibility files in a sibling directory — `Kobako::Transport` and `Kobako::Snippet` are the worked examples. Prefer adding a new file over expanding an existing one.

   **Types nest under a Module, not a Class.** A stateful Class is per-instance and should not double as the namespace for sibling types. Place new types at the top level (`Kobako::Capture`, `Kobako::Snapshot`) or under a Module (`Kobako::Transport::Request`, `Kobako::Outcome::Panic`). Do not introduce Classes nested under a Class for type-grouping purposes.

3. **Keep it simple. Don't pre-abstract.** Model exactly what SPEC requires — no speculative interfaces, parallel hierarchies, or defensive layers. Three similar lines beats a premature abstraction; a one-shot operation does not need a helper. Avoid feature flags and backwards-compatibility shims when the code can just change.

4. **Follow language community conventions via tooling.** Ruby: Rubocop + Steep. Rust: `cargo fmt` + `cargo clippy -D warnings` (clippy also under `--target wasm32-wasip1`). All four run on every Edit/Write via PostToolUse hooks; failures block the edit. When a cop or lint fires, **shrink the code to fit the tool** — don't widen `.rubocop.yml` exclusions or add `#[allow]` / `# steep:ignore`. Existing exclusions are anchored to specific SPEC-to-code mappings; add new ones only with an inline comment naming the mapping.

   **Tool-vs-tool conflicts are the one justified widening.** When Rubocop and Steep / RBS upstream disagree on the same code shape, prefer the type-system guidance and disable the cop at the `.rubocop.yml` level with a comment citing the upstream source. `Style/DataInheritance` is disabled on that basis (ruby/rbs [`docs/data_and_struct.md`](https://github.com/ruby/rbs/blob/master/docs/data_and_struct.md) documents the `class X < Data.define(...)` subclass form as the Steep-friendly pattern). `Kobako::Outcome::Panic`, `Kobako::Transport::Run`, `Kobako::SandboxOptions`, and `Kobako::Snippet::{Source,Binary}` are worked examples. Every `Data.define` type in `lib/` now uses the subclass form; keep new types on it and never reintroduce the assignment form.

5. **Document Ruby in RDoc prose.** No tool enforces this — match the existing style. Wrap identifiers in `+code+`. Cite SPEC as `{SPEC.md B-XX}[link:<relative path>]` in plain text. Do not use YARD tags (`@param` / `@return` / `@raise`); migrate them when touching nearby code.

   ```ruby
   # Host-side mapping from opaque integer Handle IDs to Ruby objects.
   # One table is owned per Sandbox and injected into Kobako::Catalog::Namespaces.
   # See {SPEC.md B-15}[link:../../../SPEC.md].
   #
   #   - {SPEC.md B-15}[link:../../../SPEC.md] — IDs are monotonically
   #     allocated per invocation; ID 0 is the invalid sentinel.
   #   - {SPEC.md B-21}[link:../../../SPEC.md] — Cap is +MAX_ID+; allocation
   #     beyond raises immediately, no wrap, no reuse.
   class Handler
     # Bind +object+ in the table and return its newly-allocated Handle ID.
     # +object+ is any host-side Ruby object to bind. Returns a freshly-
     # allocated Handle ID in +[1, MAX_ID]+. Raises
     # +Kobako::HandlerExhaustedError+ if the next ID would exceed the cap.
     def alloc(object)
       # ...
     end
   end
   ```

   **In Rust, wrap identifiers in backtick code spans (`` `Invocation` ``); do not use rustdoc intra-doc links (`` [`Invocation`] ``).** Intra-doc links rot silently — they break on renames and cannot target private items (`pub(crate)` / `pub(super)` / private `fn`), so the link either dangles or silently drops. `cargo doc --no-deps --document-private-items` under `RUSTDOCFLAGS=-D warnings` runs on both workspaces in the Stop hook and rejects any such breakage (plus stray HTML like `<u8>` — backtick those too). Backtick spans never resolve, so they never rot. Reference-style file links such as `[SPEC.md ...]: ../../SPEC.md` are not intra-doc links and stay.

6. **Route end-to-end coverage through the real mruby guest** (`data/kobako.wasm`). Do not introduce parallel fixture-driven wasm crates; if a behavior cannot be exercised through mruby, prefer a host-side unit test against `Kobako::Outcome` / `Kobako::Transport::Dispatcher` or a hand-rolled minimal wasm module (see `test/fixtures/minimal.wasm`).

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

## Layering

### Three modules across the wasm boundary

kobako is three source trees that meet at the wasm sandbox boundary. The host and guest are deliberately **wire-symmetric**; `ext/` is the driver that connects them.

```
HOST (process)                                  │  GUEST (wasm32, one Sandbox)
────────────────────────────────────────────── │ ──────────────────────────────────────
lib/  — Ruby gem, the user-facing API           │  wasm/kobako-wasm  — Rust guest
  Sandbox · Catalog · Transport · Outcome        │    abi · kobako(domain) · transport
  · Codec · Root        (tier stack below)       │    · outcome · codec · root  (mirrors lib/)
       ▲  owns the wire codec                    │       ▲  owns the wire codec
       │  (#encode / .decode, duck-typed)        │       │  (codec::{Encode,Decode} trait)
       │                                         │  wasm/mruby  — typed Rust wrapper
       │                                         │    Mrb · Value · Class · Array · Hash
       │                                         │    · IntoValue/FromValue · Format · protect
       │                                         │  wasm/mruby-sys  — bindgen FFI surface
       │                                         │    bindings::* · mrb_func_t · mrb_args_*
       │                                         │    · ABI const assertions
       │                                         │    → libmruby.a (mruby C API)
       ▼                                         │       ▲  kobako-wasm → mruby → mruby-sys
ext/  — Rust native ext (magnus + wasmtime)      │       │
  Runtime (Exports, Config) · Invocation ·       │       │
  dispatch · guest_mem · cache · Snapshot        │       │
       └─────────── drives the ABI ─── wasm ─────┼───────┘
                    (alloc / eval / run / take_outcome / dispatch / yield)
```

- **`lib/` ↔ `wasm/kobako-wasm` are wire-symmetric peers.** Each independently implements the same SPEC wire — MessagePack `Codec` plus the `Transport` / `Outcome` envelopes — so envelopes round-trip byte-for-byte (the `*_oracle` fuzz checks pin this). Every wire value object self-encodes: Ruby via duck-typed `#encode` / `.decode`, the guest via the `codec::{Encode, Decode}` trait (lives at the **codec tier** because per-call `Transport` *and* per-run `Outcome`/`Panic` implement it). The asymmetry that stays: success/failure is a value on the guest (`Outcome` enum) but a return-or-raise on the host (`Outcome.decode` is a module function) — Rust vs Ruby error models, correct on each side.
- **`ext/` is the host's wasmtime driver, not a wire endpoint.** It instantiates the guest, drives the ABI exports, and shuttles *raw bytes* between Ruby (which owns the codec) and the guest — it never decodes envelopes itself. Its internal layering mirrors the guest's `abi.rs` (packed-u64, `__kobako_alloc`, linear-memory I/O via `guest_mem`), not the codec. The Rust struct is `Runtime` (matching the `Kobako::Runtime` magnus class); `Exports` caches the per-instance export handles, `Config` holds the caps, `cache` is the process-wide Engine/Module cache.
- **`wasm/mruby` is the typed mruby C-API wrapper**, consumed by `kobako-wasm` via the `crate::mruby` façade (`pub use mruby::*`). Owns every Rust-level abstraction over the mruby C API: `Mrb` / `Ccontext` RAII, the `Value` / `Class` / `Array` / `Hash` newtypes, `convert.rs`'s `IntoValue` / `FromValue` trait seam, `state::args`'s `Format` + ZST + GAT `mrb_get_args` dispatch, `state::protect`'s closure wrapper, the typed `mrb_func_t` alias, and the `cstr!` macro. Splits magnus-style off `mruby-sys` so the FFI surface stays at the bindgen boundary.
- **`wasm/mruby-sys` is the bindgen FFI surface only**, consumed only by `mruby`. Holds the bindgen-generated `extern "C"` declarations (`mrb_open` / `mrb_load_nstring` / …), the `mrb_value::zeroed()` / `mrb_object_class` / `mrb_args_*` helpers, the host-target type placeholders, the ABI const assertions that pin `mrb_value` size / `mrb_state.exc` offset against a vendored-mruby drift, and the raw `mrb_func_t` (`mrb_value`-based) alias bindings.rs reaches via `super::mrb_func_t`. No typed wrappers — those moved to `mruby`.

### `lib/` tier stack

Dependencies point downward — a tier may use the tiers below it, never above. The non-obvious tier is the **root** of dependency-free value objects / entities: they live at `Kobako::*`, *not* under the layer that consumes them, so a lower layer can use them without an upward dependency.

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

`Codec` depends on `Root` (it registers the `Handle`/`Fault` ext types); `Transport`, `Outcome`, `Catalog`, and orchestration depend on `Codec` + `Root`.

**Placement rule (a `Codec → Transport` cycle bit us once):** a type's namespace follows **dependency direction, not which layer reads it most**. The ext-type leaves `Kobako::Handle` (0x01) and `Kobako::Fault` (0x02) are consumed almost entirely by Transport, yet they sit at the root because `Codec` — below Transport — must register them; nesting them under `Transport` would force `Codec` to depend upward. When unsure where a value object belongs, put it at the **lowest tier that needs it**.

### `ext/` tier stack (Rust native ext, host)

The wasmtime driver. Same downward-dependency rule; `runtime.rs` (module root) owns the `Kobako::Runtime` magnus class and drives the tiers below it.

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

`Kobako::Snapshot` (`src/snapshot.rs`) is the ext's **root** value object — a pure per-invocation carrier (`return_bytes` + capture + usage), depended on by `runtime` but depending on nothing. `dispatch`/`guest_mem`/`trap` all reach down to `Invocation`; `cache` sits at the floor (only the error constructors in `runtime.rs` above it).

### `wasm/kobako-wasm` tier stack (guest)

Mirrors `lib/` tier-for-tier — the wire-symmetric peer.

```
ABI entry       abi + abi/{boot, eval, run, yield_block, frames, outcome_buffer, …}
      │           __kobako_{eval,run,alloc,take_outcome,dispatch}
Domain          kobako + kobako/{install, bridges, io, codec_convert}
      │           installs the Kobako module / classes on an mrb_state
Transport ──┐   transport::{Request, Response, Yield, proxy}
Outcome ────┤   outcome::{Outcome, Panic}
      │     │
Codec ◄─────┘   codec::{Encoder, Decoder, Value, Error, Encode, Decode}   (byte-level wire)
      │
(mruby)         wasm/mruby (typed wrapper) → wasm/mruby-sys (bindgen FFI)
```

### `wasm/mruby` tier stack (typed wrapper)

```
L2 trait seams  convert::{IntoValue, FromValue}
                state::args::{Format + format::* ZST markers + GAT Output<'a>}
                state::protect (closure-based mrb_protect_error)
      │
L1 RAII /       Mrb (state.rs, NonNull<mrb_state>)
   newtypes     Value (value.rs, #[repr(transparent)] over sys::mrb_value, owns cstr!)
                Class · Module (class.rs)
                Array · Hash (array.rs / hash.rs, transparent over Value)
                Ccontext (ccontext.rs, RAII *mut mrb_ccontext)
                state::{factory, define, symbol, load} — per-concern Mrb inherent methods
      │
(FFI)           wasm/mruby-sys (path dependency, re-exported as `mruby::sys`)
```

The typed `mrb_func_t` alias at the crate root uses `Value` for receiver / return slots; `Class::define_method` transmutes it once to the raw `sys::mrb_func_t` (`mrb_value`-based) before forwarding to `sys::mrb_define_method`. The two are ABI-identical because `Value` is `#[repr(transparent)]`. `convert` sits at the **top** of the L2 trait layer, on the raw tag/box primitives on `Value`.

### `wasm/mruby-sys` tier stack (bindgen FFI surface)

```
ABI const     mrb_value::zeroed() · const assertions on mrb_value size / mrb_state.exc offset
helpers       mrb_object_class · mrb_args_{none, any, req}
      │
FFI surface   bindings::* (bindgen output, wasm32) · mrb_func_t (raw alias)
              · host placeholders (mrb_state = c_void, etc. on non-wasm32)
              · wrapper.h static-inline macro shims compiled by build.rs
```

`build.rs` is the only consumer of `MRUBY_LIB_DIR` / `WASI_SDK_PATH`; libclang stays a sys-only build dependency so the bindgen cost sits in one place. The crate exposes raw `mrb_value`, `mrb_state`, `RClass`, etc.; typed newtypes belong upstream in `mruby`.

## Where to Look

Entry points only — siblings (`outcome/panic.rb`, `snippet/{source,binary}.rb`, `transport/request.rb`, etc.) are reachable from there. The Notes column carries only what reading the entry-point file won't tell you.

| Topic | Entry points | Notes |
|-------|--------------|-------|
| Wire format / codec | host `lib/kobako/codec/`, `lib/kobako/transport/` (envelopes: `request.rb` / `response.rb` / `run.rb` / `yield.rb`); guest `wasm/kobako-wasm/src/{codec,transport}/` | Envelope shapes: `docs/wire-contract.md`. Byte-level: `docs/wire-codec.md`. Ext-type leaves are root-level: `Kobako::Handle` (0x01), `Kobako::Fault` (0x02) — see Layering. |
| Error taxonomy / outcome | `lib/kobako/errors.rb`, `lib/kobako/outcome.rb` | E-xx anchors in `docs/behavior.md`. |
| Sandbox lifecycle | host `lib/kobako/sandbox.rb`, `ext/kobako/src/runtime.rs`; guest `wasm/kobako-wasm/src/abi.rs` | `Kobako::Transport::Run` carries the `#run` host→guest envelope; guest→host dispatch arrives via the `Runtime#on_dispatch=` Proc (`lib/kobako/transport/dispatcher.rb`). B-xx in `docs/behavior.md`. |
| Guest IO / `$stdout` / `$stderr` | `wasm/kobako-wasm/src/kobako/io.rs`, `wasm/kobako-wasm/mrblib/{io,kernel}.rb` | mrblib is precompiled to RITE bytecode by `build.rs` and embedded via `src/kobako/bytecode.rs`. SPEC B-04. |
| Transport dispatch | host `lib/kobako/transport/dispatcher.rb`; guest `wasm/kobako-wasm/src/transport/` | Host dispatcher **never raises** — every failure becomes a `Response.err` envelope. |
| Catalog::Handles / capability handles | `lib/kobako/catalog/handles.rb` | B-13..B-21 in `docs/behavior.md`. Owned by Sandbox (B-19), injected into `Kobako::Catalog::Namespaces` so guest→host dispatch and host→guest wire encoding share one allocator. Per-invocation reset is the Sandbox's job — the registry holds the reference but never calls `#reset!`. |
| Service registration | `lib/kobako/catalog/namespaces.rb`, `lib/kobako/namespace.rb` | Per-Sandbox `Catalog::Namespaces` owns the `Kobako::Namespace` registry; bound objects live one level deep at `"Namespace::Member"`. Catalog::Handles is injected by the owning Sandbox, not owned by the registry. |
| ABI surface (host ↔ guest exports) | `wasm/kobako-wasm/src/abi.rs` ↔ `ext/kobako/src/runtime.rs` | — |
| E2E coverage | `test/test_e2e_journeys.rb` (`#eval`), `test/test_sandbox_run.rb` (`#run`) | Both drive real `data/kobako.wasm`. Wrapper-tier (`test/test_wasm_wrapper.rb`) covers only `from_path` and deliberately does not duplicate ABI-export checks. |
| mruby typed wrapper | `wasm/mruby/src/{state,value,class,array,hash,ccontext,convert}.rs`, `wasm/mruby/src/state/{args,factory,define,symbol,load,protect}.rs` | Owns `Mrb`, `Value`, `Class`, `Array`, `Hash`, `Ccontext`, `IntoValue`/`FromValue`, `Format`+ZST+GAT `get_args`, `protect`, the typed `mrb_func_t`, and `cstr!`. Consumed by `kobako-wasm` via the `crate::mruby` façade (single `pub use mruby::*`). Sits over `mruby-sys`. |
| mruby C API FFI | `wasm/mruby-sys/` (`wrapper.h`, `build.rs`, `src/lib.rs`) | bindgen-only surface: `extern "C"` declarations + `mrb_value::zeroed` + `mrb_args_*` + `mrb_object_class` + ABI const assertions + host placeholders. bindgen scoped here (libclang stays sys-only); `wrap_static_fns` emits a single C trampoline — no hand-written `.c` shims. Consumed only by the `mruby` wrapper crate. |
| RBS signatures | `sig/kobako/` (mirrors `lib/kobako/` 1:1) | Three sources stack: `sig/_external/` (hand-rolled), `rbs_collection.{yaml,lock.yaml}` (gem), and `library "<name>"` in `Steepfile` (stdlib — reach for this first). PostToolUse steep hook blocks Ruby edits without matching `.rbs`. |
| Regression benchmarks | `tasks/benchmark.rake`, `benchmark/` | #1..#5 are gated (+10% regression blocks release); #6/#7 are characterization, not gated. Results: `benchmark/results/<date>-<short-sha>.json`. Scope + caveats in `benchmark/README.md`. |
| Build / toolchain | `tasks/{vendor,mruby,wasm}.rake` | — |

`test/test_helper.rb` rescues `LoadError` when `lib/kobako/kobako.bundle` is missing and stubs `Kobako::Error`, so the suite still loads on a clean checkout; individual tests `skip` themselves when the native ext is absent.
