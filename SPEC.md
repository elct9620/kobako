# SPEC.md — kobako

## Intent

Purpose, users, impacts, and non-goals are stated in [`docs/intent.md`](docs/intent.md); every concept is named in [`docs/spec/glossary.md`](docs/spec/glossary.md).

---

## Refinement

### Wire

The host↔guest wire is specified in [`docs/wire-contract.md`](docs/wire-contract.md) (the abstract shape) and [`docs/wire-codec.md`](docs/wire-codec.md) (the two encoding layers, the ABI surface, and the consistency guarantee).

---

### Implementation Standards

#### Architecture

The kobako codebase is split into top-level source areas with a strict boundary between them:

- **`lib/`** — the Host Gem Ruby surface. Contains `kobako.rb` (the main entry point that loads the native extension and defines the public API) and `lib/kobako/` sub-modules (error class definitions, codec helpers, Transport value objects and Dispatcher, Catalog registries — `Services` / `Snippets` / `Handles`). This is the only layer the Host App interacts with directly.
- **`ext/kobako/`** — the private native extension (the `kobako` Rust crate). A thin magnus shim over the host crates in `crates/`: it registers the Ruby classes, bridges the dispatch Proc, and maps the neutral error channels onto the `Kobako::*` classes. This is a private implementation detail of the Host Gem; it is never intended as a reusable wasmtime binding and exposes no Wasm engine types to the Host App or downstream gems.
- **`crates/`** — the publishable Rust crates a non-Ruby embedder builds on: `kobako-transport` (the fixed tier — the core envelope and the ABI surface, shared by both sides of the wasm boundary and depending on nothing), `kobako-codec` (the payload codecs, one namespace per schema), `kobako-runtime` (the engine-neutral runtime contract), and `kobako-wasmtime` (the wasmtime driver — owns the Wasm engine lifecycle and implements the host-side import function `__kobako_dispatch`). The ext consumes `kobako-transport`, `kobako-runtime`, and `kobako-wasmtime` as path dependencies; the guest crates consume `kobako-transport` the same way across the workspace boundary, and `kobako-codec` only when they speak its schema.
- **`wasm/`** — the Guest Binary source (`kobako-wasm` Rust crate, target `wasm32-wasip1`). This is build-time only; it is compiled to `data/kobako.wasm` and excluded from the published gem alongside build tools (`vendor/`, `tasks/`, `build_config/`).
- **`data/kobako.wasm`** — the pre-built Guest Binary artifact. Produced at release time on the publisher's machine and shipped inside the gem. End users receive this file at install time; they never need to recompile the Wasm side.

The boundary rule is: **`ext/` is private to the Host Gem and must never be imported by downstream gems**; `lib/` is the stable public surface. Three Cargo workspaces keep the dependency graphs separate: the root workspace (`ext/kobako` only), `crates/` (the publishable crates), and `wasm/` (the guest). The root `Cargo.toml` excludes `wasm/`, `vendor/`, and `crates/` — the ext and the guest workspace consume `crates/` members as plain path dependencies (`kobako-transport` and `kobako-codec` are engine-free, so their presence in the guest graph pulls in no host-only code), and the isolation prevents host-only crates (e.g., `wasmtime`) from appearing in the wasm32 dependency graph.

#### Code Organization

The following directory layout principles govern the repository. The specific test framework, benchmark library, and CI provider are implementation choices and are not pinned here.

**Directory roles (required, not relocatable):**

- `lib/` — Host Gem Ruby surface; public API entry point and sub-modules
- `ext/kobako/` — private native extension; Rust source (`src/`), `Cargo.toml`, `extconf.rb`; compiled to `lib/kobako/kobako.<ext>` by rake-compiler
- `crates/` — every Rust crate that is not wasm-only, so a Rust embedder and the guest crates can each reach it. Only the ext's path-dependency closure (`kobako-transport`, `kobako-runtime`, `kobako-wasmtime`) ships in the published gem; the `crates/` workspace manifest and lock never do
- `wasm/` — Guest Binary Rust source; compiled to `data/kobako.wasm`; excluded from the published gem
- `data/` — pre-built Wasm artifact (`kobako.wasm`); included in the published gem; never manually edited
- `build_config/` — mruby build configuration (`wasi.rb`); build-time only; excluded from the published gem
- `vendor/` — build-time toolchain storage for wasi-sdk and mruby tarballs; not committed; entirely covered by `.gitignore`; excluded from the published gem
- `tasks/` — Rakefile sub-task files, each owning one task group and self-contained enough to be loaded by glob; the Rakefile is the list, so a group is added by adding a file. Excluded from the published gem
- `test/` — every test file, whatever its kind; excluded from the published gem
- `benchmark/` — benchmark scripts and baseline result files; excluded from the published gem
- `docs/` — design documentation; excluded from the published gem

**gemspec files whitelist:** `kobako.gemspec` pins `spec.files` so the published gem contains exactly the install surface: `lib/**/*.rb`, `ext/kobako/**`, the ext's crate path-dependency closure (`crates/kobako-transport/**`, `crates/kobako-runtime/**`, `crates/kobako-wasmtime/**` — never the `crates/` workspace manifest or lock, and a new crate stays out until the ext depends on it), `data/kobako.wasm`, `sig/**` (minus the dev-only `sig/_external/`), `README.md`, `LICENSE`, `CHANGELOG.md`. All other directories (`vendor/`, `wasm/`, `tasks/`, `build_config/`, `docs/`, `benchmark/`, `test/`) are excluded.

**Two build paths, two starting points:**

- *End-user path*: `gem install kobako` → rake-compiler runs `compile_ext` (Rust toolchain required) → `data/kobako.wasm` is already present; wasi-sdk and mruby tarballs are not needed.
- *Developer path*: `git clone` → `bundle install` → `bundle exec rake compile` → the `beni` gem's tasks vendor the pinned wasi-sdk and mruby into `vendor/` and build `libmruby.a`, then this repository's wasm build links `data/kobako.wasm`, then `compile_ext`.

Every build task must be idempotent: the presence of the target file its stage produces short-circuits re-execution, so incremental development only reruns the changed stage. This holds across the boundary — a stage the toolchain gem owns is as re-entrant as one this repository owns.

**Release documentation — six required artifacts:** A release is not complete until all six of the following documents are present and synchronized with the code. Shipping code before documentation is not permitted.

| # | Document | Contents |
|---|----------|----------|
| 1 | `README.md` | Quickstart (5-line runnable example), API overview, install flow including MSRV |
| 2 | Development guide (`docs/`) | Complete design specification (this document) |
| 3 | Wire Spec | Normative host↔guest codec contract; the binding reference for the Host Gem and Guest Binary implementations shipped in this release |
| 4 | Build guide | Rake task reference, vendor version table, common build error troubleshooting |
| 5 | `CHANGELOG.md` | Keep a Changelog format, generated and maintained by release-please from Conventional Commit messages — never hand-authored. release-please opens a release PR that writes the file and derives its Added / Changed / Fixed / Breaking Changes sections from the `feat` / `fix` / `feat!` / `BREAKING CHANGE:` commit types since the last release; the file first appears with that release PR. |
| 6 | `LICENSE` | License file |

Wire-affecting changes that break round-trip compatibility are recorded by marking their commit as a breaking change (`feat!` / `fix!` or a `BREAKING CHANGE:` footer); release-please rolls these into the CHANGELOG's Breaking Changes section automatically. MSRV changes are treated as breaking changes and marked the same way. The contributor's obligation is the commit-message convention, not editing `CHANGELOG.md` directly.
