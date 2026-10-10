# AGENTS.md

Guidance for coding agents in this repository. It keeps the conventions the source cannot show, how we work, and an index to everything else, which is left where it lives.

| Section | Answers |
|---|---|
| Project | what kobako is made of |
| How we work | the decisions every change follows |
| Architecture | where the structure is drawn, and how a change crosses a layer |
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
| a contract only prose can hold, such as a byte layout | `docs/wire/` |
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
| a document by reader | gem or crate users in `docs/guides/`, maintainers in `docs/` |
| a crate README | its own crate only; no link into the repository's docs |

Braces or backticks stop RDoc from linking a name, so neither is used; `lib/kobako/catalog/handles.rb` is the worked example. Intra-doc links rot silently and cannot reach private items. A published README is frozen per version, so a link into `docs/` breaks for good once a document moves. A rewritten Markdown section is brought to zero lint errors.

### 6. Tests

Every Ruby test lives under `test/`, grouped by kind, and `tasks/` holds none.

| Rule | Instead of |
|---|---|
| end-to-end through the real `data/kobako.wasm` | a parallel fixture wasm crate |
| host-side unit test, or `test/fixtures/minimal.wasm` | a behavior mruby cannot reach |
| a test's home follows what it needs | a home picked by subject alone |
| paths and skips via `TestPaths` and `GuestGuard` | hand-rolled `__dir__` paths or guards |

A scenario test's assertion message is a contract: "<input> through <public API> must <behaviour>". One that lacks it gains it the next time the test is touched. Witness rationale goes in the comment above the test; `test/e2e/test_io_write.rb` is the worked example.

## Architecture

`docs/architecture.md` draws the crate map, the Ruby tiers, and the placement rules that keep those tiers acyclic. Read it before adding a type or a crate.

```
lib/  Orchestration → Catalog → Transport · Outcome → Payload → Codec → Root
```

A Ruby tier uses only the tiers to its right. These rules govern how a change crosses a layer.

| Rule | Why |
|---|---|
| wrapper changes go to `beni` upstream | kobako takes them by a dependency bump |
| a low-level gap is fixed in that layer | a workaround above leaves it for every other caller |

## Build and commands

### Build chain

The Guest Binary is gitignored and built in three stages.

1. Run `rake beni:build` for Stages A and B, driven by `build_config/wasi.rb`.
2. Run `rake wasm:build` for Stage C, which ends with the `kobako-baker` bake.
3. Run `rake compile` from a clean clone to walk the chain and build the native ext.

The gem bundles only the pure default; capability variants are Release assets described in `docs/guides/variants.md`.

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
| wire format | `docs/wire/README.md` | anchors the envelope and payload layers |
| core envelope | `crates/kobako-transport/src/` | one implementation for both sides |
| host payload codec | `lib/kobako/{codec,payload}/` | a Fault rides the envelope instead |
| guest payload codec | `crates/kobako-codec/src/msgpack/` | wire-symmetric peer of `lib/` |
| vocabulary | `docs/spec/glossary.md` | a later term replaces an earlier one |
| sandbox lifecycle | `lib/kobako/sandbox.rb` | each run settles into an `Execution` |
| dispatch | `lib/kobako/transport/dispatcher.rb` | answers an ok or fault triple, never raises |
| Handles | `lib/kobako/catalog/handles.rb` | minted per invocation by `Context` |
| Extensions | `docs/guides/extensions.md` | a backend's kind is a keyword |
| guest capabilities | `wasm/kobako-{io,regexp,json}/src/` | pure-Rust `beni::Gem`, no mrblib |
| ABI surface | `wasm/kobako-core/src/guest.rs` | bodies in `wasm/kobako-mruby/src/flows.rs`; host side in `crates/kobako-wasmtime/src/driver.rs` |
| security | `docs/guides/security.md` | the host is the boundary |
| customization | `docs/guides/customization.md` | grades are commitments |
| parity | `docs/parity.md` | behavior aligns; APIs stay idiomatic |
| RBS | `sig/kobako/` | reach for a stdlib `library` first |
| benchmarks | `benchmark/README.md` | `support/roster.rb` names the gated set |
| examples | `examples/` | pin the released gem, not `main` |
| releases | `docs/releasing.md` | read before `Release-As` or `!` |
