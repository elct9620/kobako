# AGENTS.md

Guidance for coding agents in this repository. It holds only the decisions the source cannot show.

## Intent

kobako is a Ruby gem that runs untrusted mruby scripts in an in-process Wasm sandbox. Why it exists and for whom is in `docs/intent.md`.

## How We Work

Every change follows these rules. An earlier row wins on conflict.

| Topic | Rule |
|---|---|
| Spec | sumi under `docs/spec/` holds behavior; extend it first when silent |
| Spec | Each scenario owes a test tagged `@behavior`; prose cites no scenario id |
| Spec | A contract's `Includes` lists declaring files only, never append-only ones |
| Code | Build what the spec asks; no flags, shims, or defensive layers |
| Code | `vfs` aligns behavior and logic to Linux VFS; `filesystem` follows the spec |
| Code | Fix a gap in its own layer; wrapper changes go to `beni` upstream |
| Tooling | `bundle exec rake` is the gate |
| Tooling | Fit the code to a lint; widen only for an RBS conflict or wiggle output |
| Tooling | Reach for a stdlib RBS `library` first |
| Docs | State what and why, never mechanism |
| Docs | Ruby: RDoc prose, no YARD tags; Rust: backticks, no intra-doc links |
| Docs | A crate README links nothing in `docs/` |
| Tests | Drive the real `data/kobako.wasm`; build no fixture wasm crate |
| Tests | A scenario test's message: "<input> through <public API> must <behaviour>" |
| Tests | A helper's failure states what decides the next step |

These commands are missing from `rake -T`.

| Task | Command |
|---|---|
| One test file | `bundle exec ruby -Ilib -Itest <file>`, plus `-n /pattern/` for one test |
| Check against the spec | `sumi verify` |
| Where a scenario is declared and claimed | `sumi inspect <id>`, or `<path:line>` for the reverse |
| What each feature's tests witness | `sumi stats` |
| One module's statistics | `rake stats:<module>` |

Some changes move together.

| When this changes | Also update |
|---|---|
| A behavior or an interface | The scenario or contract first; `sumi verify` reports no difference |
| A file that declares a public interface | The contract's `Includes` |
| A type, a crate, or a layer | `docs/architecture.md` |
| A dependency | Every workspace's lock file, in the same commit |
| A release | `examples/` pins move to it; read `docs/releasing.md` before `Release-As` or `!` |

## Index

Start here to grasp the whole before reading code.

| Path | What you learn here |
|---|---|
| `docs/intent.md` | Why kobako exists, and for whom |
| `docs/architecture.md` | The crate map, the seams, the Ruby tiers, and the placement rules |
| `docs/spec/` | Behavior, contracts, and the glossary; the source of truth |
| `docs/convention.md` | The naming baseline a rename is judged against |
| `docs/wire/README.md` | The envelope and payload layers on the wire |
| `docs/guides/` | What a Host App relies on: security, customization, extensions |
| `lib/kobako/sandbox.rb` | Where a run starts and settles into an `Execution` |
| `crates/kobako-runtime/src/` | Every answer a guest sees: the VFS, the Policy, the Filesystems |
| `crates/kobako-wasmtime/src/preview1.rs` | How the Driver encodes WASI preview1 into runtime calls |
| `wasm/kobako-core/src/guest.rs` | The ABI surface the Guest Binary exports and imports |
