# CLAUDE.md

Guidance for Claude Code in this repository. It keeps only the decisions no tool, file, or convention already states; everything else is left where it lives.

| Section | Answers |
|---|---|
| Project | what kobako is made of |
| How we work | the decisions every change follows |
| Architecture | the rules the crate map does not show |
| Build and commands | what `rake -T` cannot tell you |
| Entry points | where each topic starts |

## Project

kobako is a Ruby gem that runs untrusted mruby scripts in an in-process Wasm sandbox. The host drives a precompiled Guest Binary over a MessagePack-based wire.

| Part | Lives in | Role |
|---|---|---|
| Host gem | `lib/`, `ext/` | Ruby API and the magnus shim |
| Native driver | `crates/` | wasmtime driver and the Rust SDK |
| Guest Binary | `wasm/` → `data/kobako.wasm` | the mruby interpreter |
| Specification | `docs/spec/` | the source of truth |

## How we work

Apply these in order; an earlier one wins on conflict.

| # | Area | The decision in one line |
|---|---|---|
| 1 | Specification | sumi holds behavior; `@behavior` is the only link |
| 2 | Structure | one thing per file; types nest under a Module |
| 3 | Simplicity | build what the spec asks; converge while polishing |
| 4 | Tooling | shrink the code to fit the tool |
| 5 | Documentation | state intent, never mechanism |
| 6 | Tests | drive the real guest; messages are contracts |

### 1. Specification

The specification is sumi's corpus under `docs/spec/`. What it cannot hold is context for readers, kept in the document that owns it.

| Statement | Lives in |
|---|---|
| a concept and its name | `docs/spec/glossary.md` |
| a behavior | a scenario in `docs/spec/behavior/<feature>.md` |
| an interface | `docs/spec/contract/` |
| why kobako exists, and for whom | `docs/intent.md` |

A behavior is specification only as a scenario. Prose never restates one. A behavior no test can reliably witness carries an `unverifiable` row naming why. One merely untested stays out until a test claims it. A test claims its scenario with `@behavior`, the one traceability link; docs, examples, and comments cite no scenario id.

The glossary carries concepts, not their Ruby or Rust spelling. Never rule out a word that also names a concept of ours, such as `Execution` beside `Invocation`. Keep `Includes` off append-only files, whose shifting lines would strand an `ignore`. When the specification is silent, extend it first.

### 2. Structure

A growing module splits into a façade plus per-responsibility files in a sibling directory.

| Rule | Worked example |
|---|---|
| façade plus sibling directory | `Kobako::Transport`, `Kobako::Snippet` |
| new types at the top level | `Kobako::Capture`, `Kobako::Usage` |
| or nested under a Module | `Kobako::Payload::Arguments` |

A stateful Class is per-instance, so it never doubles as the namespace for sibling types. Where a type sits in `lib/` follows the placement rules under Architecture.

### 3. Simplicity

The feature set has converged, so the ideal change keeps behavior identical with less implementation.

| Situation | Do |
|---|---|
| building | model exactly what the spec requires |
| polishing | same behavior, less code, no surface change |
| pruning | lock external interfaces first, then prune behind them |
| changing | change the code; no feature flags or back-compat shims |

No speculative interfaces, parallel hierarchies, or defensive layers. Existing tests pin the behavior a pruning step must keep.

### 4. Tooling

Every edit and every stop runs the checks in `.claude/settings.json`. When one fires, shrink the code to fit it.

| Situation | Do |
|---|---|
| a cop or lint fires | change the code |
| tempted to widen | never add exclusions, `#[allow]`, or `# steep:ignore` |
| RuboCop and RBS disagree | disable the cop in `.rubocop.yml`, citing upstream |
| a dependency changes | commit every workspace's lock file with it |

The tool-vs-tool case is the one justified widening, and the type system wins it. Lock files ship even for the gem, unlike the usual gem convention.

### 5. Documentation

A doc or comment answers what and why in one or two sentences; mechanism is what the code already shows.

| Where | Rule |
|---|---|
| Ruby | RDoc prose, identifiers in `+code+`, no YARD tags |
| Ruby cross-references | written bare, so RDoc links them |
| Rust | identifiers in backticks, no intra-doc links |
| a list that will drift | intent plus a pointer to its owner |
| human-facing Markdown | concise-docs rules; `docs/spec/**` keeps sumi's form |

Braces or backticks stop RDoc from linking a name, so neither is used; `lib/kobako/catalog/handles.rb` is the worked example. Intra-doc links rot silently and cannot reach private items. A rewritten Markdown section is brought to zero lint errors.

### 6. Tests

Every Ruby test lives under `test/`, grouped by kind, and `tasks/` holds none.

| Rule | Instead of |
|---|---|
| end-to-end through the real `data/kobako.wasm` | a parallel fixture wasm crate |
| host-side unit test, or `test/fixtures/minimal.wasm` | a behavior mruby cannot reach |
| a test's home follows what it needs | a home picked by subject alone |
| paths and skips via `TestPaths` and `GuestGuard` | hand-rolled `__dir__` paths or guards |

An assertion message is a contract: "<input> through <public API> must <behaviour>". Witness rationale goes in the comment above the test; `test/e2e/test_io_write.rb` is the worked example.

## Architecture

The crate map, each crate's role, and its dependencies are drawn in `docs/architecture.md`. This section keeps the rules that map leaves out.

| Rule | Why |
|---|---|
| guest crates link libmruby on every build | no code hides behind a linked-only `cfg` |
| wrapper changes go to `beni` upstream | kobako takes them by a dependency bump |
| a low-level gap is fixed in that layer | a workaround above leaves it for every other caller |
| `Kobako::Codec` has no schema namespace | Ruby is fixed to MessagePack and has no seam |

### Ruby tiers

Inside `lib/`, a tier may use the tiers below it and never one above.

```
Orchestration   Sandbox, Pool, Runtime (+ ext), Context
      │
Catalog         setup-time registries + the per-invocation Handle table
      │
Transport ──┐   call value objects + dispatch
Outcome ────┤   guest-result attribution
      │     │
Payload ◄───┤   the [args, kwargs] shape a Call or Run carries
      │     │
Codec ◄─────┘   byte-level payload wire
      │
Root            dependency-free value objects and error classes
```

The core envelope has no tier here, because the native side frames and decodes it.

### Placement rules

These three rules keep the tiers acyclic; each was learned from a cycle or a leak.

| Rule | Example |
|---|---|
| a type sits at the lowest tier that needs it | `Kobako::Handle` at the root, for `Codec` |
| `Outcome` may require `transport/error.rb` | the gem contract fixes the class name |
| `Codec.track_handles` wraps only the decode call | wider leaks its flag into re-entry |

Namespace follows dependency direction, not which tier reads a type most. Do not move `Kobako::Transport::Error` to remove the lateral edge; its file depends only on root `errors.rb`.

## Build and commands

### Build chain

The Guest Binary is gitignored and built in three stages.

1. Run `rake beni:build` for Stages A and B, driven by `build_config/wasi.rb`.
2. Run `rake wasm:build` for Stage C, which ends with the `kobako-baker` bake.
3. Run `rake compile` from a clean clone to walk the chain and build the native ext.

The gem bundles only the pure default; capability variants are Release assets described in `docs/variants.md`.

### Release gate

`bundle exec rake` is the gate CI runs, and the Stop hook runs it too.

| Part | Rule |
|---|---|
| default task | compile, test, `crates:test`, rubocop, steep, `gate` |
| `gate` | lists every `gate:*` check, in one place |
| `crates:test` | in the default; it holds the envelope's only pin |
| `wasm:test` | a separate CI step |

The default task and CI reference `gate`, never its members, so joining it stays deliberate.

### Commands

`rake -T` is the catalog; these are the entry points it does not show.

| Task | Command |
|---|---|
| one test file | `bundle exec ruby -Ilib -Itest <file>` |
| one test by name | append `-n /pattern/` |
| check against the specification | `sumi verify` |
| one module's statistics | `rake stats:<module>`, hidden from `rake -T` |

## Entry points

Each row names an entry point and only what reading it will not tell you.

| Topic | Entry point | Note |
|---|---|---|
| wire format | `docs/wire-codec.md` | anchors the envelope and payload layers |
| core envelope | `crates/kobako-transport/src/` | one implementation for both sides |
| host payload codec | `lib/kobako/{codec,payload}/` | a Fault rides the envelope instead |
| guest payload codec | `crates/kobako-codec/src/msgpack/` | wire-symmetric peer of `lib/` |
| vocabulary | `docs/spec/glossary.md` | a later term replaces an earlier one |
| sandbox lifecycle | `lib/kobako/sandbox.rb` | each run settles into an `Execution` |
| dispatch | `lib/kobako/transport/dispatcher.rb` | answers `[ok, bytes]`, never raises |
| Handles | `lib/kobako/catalog/handles.rb` | minted per invocation by `Context` |
| Extensions | `docs/extensions.md` | a backend's kind is a keyword |
| guest capabilities | `wasm/kobako-{io,regexp,json}/src/` | pure-Rust `beni::Gem`, no mrblib |
| ABI surface | `wasm/kobako-core/src/guest.rs` | bodies in `flows.rs` and `driver.rs` |
| security | `docs/security-model.md` | the host is the boundary |
| customization | `docs/customization.md` | grades are commitments |
| parity | `docs/parity.md` | behavior aligns; APIs stay idiomatic |
| RBS | `sig/kobako/` | reach for a stdlib `library` first |
| benchmarks | `benchmark/README.md` | `support/roster.rb` names the gated set |
| examples | `examples/` | pin the released gem, not `main` |
| releases | `docs/releasing.md` | read before `Release-As` or `!` |
