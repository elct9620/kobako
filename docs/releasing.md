# Releasing

Two independent release tracks run off one `release-please` config. It reads the conventional-commit history since each track's last release and opens a release PR.

| Track | Packages | Tag | Registry |
|-------|----------|-----|----------|
| Gem | `.` (componentless) | `vX.Y.Z` | RubyGems |
| Linked crate group | the `wasm/` and `crates/` packages the config lists | `<component>-vX.Y.Z` | crates.io |

The paths a commit touches decide which track it drives; its type and breaking marker decide the bump. Merging the release PR is the only irreversible step: RubyGems has no repush, and crates.io is yank-only. The `linked-versions` plugin in `release-please-config.json` is the authoritative list of crates in the group.

## Gem Package

The gem package `.` is configured so a gem-only release still gets its tag and its version file.

| Setting | Effect |
|---|---|
| no `component` | a gem-only release still creates its `vX.Y.Z` tag |
| `version-file` | the Ruby strategy finds `lib/kobako/version.rb` |
| no `package-name` | the component stays unset |

With `include-component-in-tag: false`, release-please tags a standalone release only when the configured component matches the componentless PR section. Naming a component therefore silently drops the tag. Without a component, the Ruby strategy cannot find `VERSION` on its own, so `version-file` names it. Dropping `version-file` releases the previous version while the tag and CHANGELOG move on. `package-name` would also locate the file, but it sets the component and loses the tag again.

## Track Selection

The gem package `.` claims every root change except `exclude-paths: ["wasm", "crates"]`; each crate package claims its own subtree.

| A commit that touches… | Triggers |
|------------------------|----------|
| only root files (`lib/ ext/ sig/ test/ docs/ README.md examples/ …`) | Gem |
| a listed crate package's path | Linked crate group |
| both root and a listed crate path | Both |
| only an unlisted path under `wasm/` or `crates/` | Neither |

One crate in the group releasing syncs every other to the same version. A commit touching both tracks releases both, so avoid it unless a coordinated dual release is intended. `wasm/kobako-wasm` and `crates/kobako-parity` are not packages, so a change confined to them releases nothing.

`exclude-paths` must stay symmetric. Both `wasm/` and `crates/` have a workspace-root `Cargo.lock` and `Cargo.toml` that no package claims. Excluding only one lets crate-only work leak into the gem.

## Version Bumps

Standard semver applies once the project is stable (≥ 1.0).

| Type | Bump |
|------|------|
| `feat:` | minor |
| `fix:` | patch |
| any type with `!` or a `BREAKING CHANGE:` footer | major |
| unmarked `refactor` `docs` `test` `chore` `build` `ci` `perf` `style` | none |

An unmarked `refactor`, `docs`, or `test` commit never releases, so a change shipped only under those types is invisible to `release-please`. The breaking marker overrides the type: a `refactor!` both releases and reaches the changelog.

### Pre-1.0 Override

While the project is in 0.x, the config sets `bump-minor-pre-major: true` at the top level, so both tracks inherit it.

| Version | A `!` bumps |
|---|---|
| 0.x, with the flag | minor |
| 0.x, without the flag | straight to `1.0.0` |
| ≥ 1.0, flag removed | major |

The flag is a stabilization-period device. Remove it uniformly when the project adopts 1.0.

## Release Situations

Most releases need nothing extra; the rows below cover the ones that do.

| Situation | Approach |
|-----------|----------|
| a `feat` or `fix` on the package's paths | nothing extra; the PR opens itself |
| a real change landed only as unmarked types | give it a trigger |
| both tracks must publish together | both triggers in one scan window |
| a breaking change | see Breaking Changes |

A catch-up trigger is preferably a genuine `feat` or `fix` touching that track's paths; correcting a stale README example is a real `fix`. Only when no natural one exists, use a `Release-As: X.Y.Z` footer under the rules below.

The tracks publish together when the gem bundles a new guest: a gem on the old guest mispairs with new crates. Put each trigger in the same scan window, path-scoped to its own track. `release-please` opens one combined PR, and a single merge publishes both.

## Breaking Changes

kobako counts a wire change that breaks round-trip compatibility, and a raised MSRV, as breaking. The marker is all a reader upgrading across the release is handed, so these rules decide whether that reader has a migration path.

| Rule | Guards against |
|------|----------------|
| mark the track whose public surface broke | a break announced on the wrong track |
| carry the migration in a `BREAKING CHANGE:` footer | a note that names no spelling |
| keep a breaking commit inside one track's paths | a crate break read as a gem break |

A change can break one track and not the other. Renaming `Kobako::Member` to `Kobako::Proxy` broke gem-user scripts, since guest scripts are gem-user-facing, but left every crate's Rust API intact.

With `!` alone, the changelog note is the commit subject, and the body is never read. The footer should name the old spelling and the new one.

`exclude-paths` scopes the commit, not the marker. A commit touching both root and `wasm/` or `crates/` announces its break on both tracks.

## Release-As Rules

Use `Release-As` only when a track has no natural `feat` or `fix` to release.

| Rule | Why |
|------|-----|
| the `Release-As: X.Y.Z` footer triggers | the `release-as` config key alone never does |
| one `Release-As` per scan window | the footer applies to every releasing component |
| never pair it with `--allow-empty` | an empty commit escapes `exclude-paths` |
| keep its files within one track's paths | `exclude-paths` then scopes the footer |

The config key only overrides a version once a release is already happening. An empty commit touches no path, so it forces the version on every component, the gem included. For the crate track, keep the commit to `wasm/` and `crates/` packages; for the gem, to root files. Touching a version snippet in each of the track's READMEs is the canonical vehicle.

## Merge Checklist

Check these in the release PR before merging, since the merge cannot be undone.

| Check | Guards against |
|-------|----------------|
| read `.release-please-manifest.json` in the diff | a track dragged to the wrong version |
| confirm each track's version matches intent | a silent bump error |
| confirm `examples/` were not the release vehicle | examples broken against their pins |
| read the generated ⚠ BREAKING CHANGES section | a note a reader cannot act on |

A wrong version usually means the gem jumped to `1.0.0` or one track inherited the other's `Release-As`.

`examples/` pin released versions: the Ruby examples pin the gem (`"~> 0.26.0"`) and the Rust examples pin crates (`"0.16"`). Switching them to a new idiom before that version publishes breaks them, so update `examples/` after the release and then bump their pins.

The BREAKING CHANGES section is the last point at which a subject-only note can get its migration text. Rewriting it in the PR is safe while nothing else lands, since `release-please` regenerates the branch on every push.

## Adding a crate to the linked group

A crate joins the group by taking every seat below. None fails where it was missed: three fail silently, and `rake gate:release:wiring` holds those three. The loud ones surface at release time, some part-way through a group publish that has already put crates on crates.io.

| Seat | What it takes | If it is missed |
|------|---------------|-----------------|
| config `packages` | the package entry and its `extra-files` | the crate never releases |
| config `linked-versions` | the component name | the crate keeps its own version |
| each dependent's `extra-files` | a pin on the new crate's version | dependents require a stale version |
| `.release-please-manifest.json` | the path at the group's version | silent: never joins the bump |
| the crate's `README.md` | an annotated version line | silent: the version never updates |
| each sibling `[dev-dependencies]` | a path and no version | silent: the lockfile sync fails |
| `.github/scripts/publish-crates.sh` | a `publish_crate` call before dependents | publish fails part-way through |
| `.github/workflows/release-please.yml` | the output, the OR chain, the `cargo update -p` lists | the lockfile sync skips it |
| crates.io | a `0.0.0` placeholder | publish fails: the name is unknown |
| crates.io Trusted Publishing | both release workflows | publish cannot authenticate |

### Seat Details

The seats that hide a detail are spelled out here.

| Seat | Detail |
|---|---|
| package entry | `component`, `release-type: "rust"`, an `extra-files` entry per lockfile plus the README |
| dependent pin | `$.dependencies['<name>'].version` in each dependent's `Cargo.toml` |
| manifest | an absent package reads as never released |
| README line | ends in `# x-release-please-version`, beside its siblings' |
| dev-dependency | cargo strips a version-less path dev-dependency when packaging |
| workflow | `<name>_release_created`, the `release-crate` job's OR chain |

The generic updater replaces a version only on an annotated line; with none, it reports success and changes nothing. No updater rewrites `[dev-dependencies]`, so a version named there stays at the last release and the lockfile sync cannot resolve it. A missed workflow seat still leaves `extra-files` writing the right version.

### Placeholder Crate

The placeholder reserves the name, and Trusted Publishing is configured against it, so it comes first.

1. Create it outside the repository, since `cargo new` inside a workspace edits that workspace's manifest.
2. Give it version `0.0.0`, a description of the real crate, `license`, and `repository`.
3. Give it a `src/lib.rs` holding only a doc comment.
4. Publish it by hand.

The real version then publishes through the normal release. `already_published` checks a specific version, so the placeholder never causes one to be skipped.

### Trusted Publishing

Trusted Publishing matches on the OIDC `workflow_ref` claim, which names the workflow that was triggered, never one reached through `workflow_call`.

| Path | Triggered workflow |
|---|---|
| automated release | `release-please.yml`, calling `release-crate.yml` |
| `on: release` or manual dispatch | `release-crate.yml` |

A config naming only one of the two authenticates on only one path, so it names this repository and both workflows.
