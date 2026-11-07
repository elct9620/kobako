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
| top-level `extra-files` | each gem release moves the ext crate's version in `ext/kobako/Cargo.toml` and the root `Cargo.lock` |

Every crate package sets its own `extra-files`, which replaces the top-level list, so only the gem package inherits it.

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
| `fix:` `perf:` | patch |
| any type with `!` or a `BREAKING CHANGE:` footer | major |
| unmarked `refactor` `docs` `test` `chore` `build` `ci` `style` | none |

A commit releases only when its type has a changelog section. The config overrides no `changelog-sections`, so `release-please`'s defaults apply: `perf` gets Performance Improvements, while an unmarked `refactor`, `docs`, or `test` commit never releases. A change shipped only under those types is invisible to `release-please`. The breaking marker overrides the type: a `refactor!` both releases and reaches the changelog.

`release-please` reads only the newest `commit-search-depth` commits, 500 by default. A release whose window holds more drops the oldest from its changelog, so the config raises the depth to 1000. Its `last-release-sha` ends every scan before a fork merge whose subject the commit parser rejects, which would leave a release untagged.

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
| a `feat`, `fix`, or `perf` on the package's paths | nothing extra; the PR opens itself |
| a real change landed only as unmarked types | give it a trigger |
| both tracks must publish together | both triggers in one scan window |
| a breaking change | see Breaking Changes |

A catch-up trigger is preferably a genuine `feat`, `fix`, or `perf` touching that track's paths; correcting a stale README example is a real `fix`. Only when no natural one exists, use a `Release-As: X.Y.Z` footer under the rules below.

The tracks publish together when the gem bundles a new Guest Binary: a gem on the old Guest Binary mispairs with new crates. Put each trigger in the same scan window, path-scoped to its own track. `release-please` opens one combined PR, and a single merge publishes both.

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

Use `Release-As` only when a track has no natural `feat`, `fix`, or `perf` to release.

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

After the merge, each native gem is installed on its own host and runs one Invocation before publishing. A gem that fails there stops the release before RubyGems.

`examples/` pin the latest released versions. The Ruby examples pin the gem to its patch series (`~> X.Y.0`); the Rust examples pin crates to their minor (`X.Y`). Switching them to a new idiom before that version publishes breaks them, so update `examples/` after the release and then bump their pins.

The BREAKING CHANGES section is the last point at which a subject-only note can get its migration text. Rewriting it in the PR is safe while nothing else lands, since `release-please` regenerates the branch on every push.

## Linked Group Seats

A crate joins the group by taking every seat below. A missed seat never fails at the step that missed it. The three marked silent report success anyway, and `test/tasks/test_release_wiring.rb` checks those three. The others fail at release time, some part-way through a group publish that has already put crates on crates.io.

| Seat | What it takes | If it is missed |
|------|---------------|-----------------|
| config `packages` | the package entry and its `extra-files` | the crate never releases |
| config `linked-versions` | the component name | the crate keeps its own version |
| each dependent's `extra-files` | a pin on the new crate's version | dependents require a stale version |
| `.release-please-manifest.json` | the path at the group's version | silent: never joins the bump |
| the crate's `README.md` | an annotated version line | silent: the version never updates |
| each sibling `[dev-dependencies]` | a path and no version | silent: the version falls behind until the lockfile sync cannot resolve it |
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
| workflow | a short `<alias>_release_created` output, such as `sdk_`, in the `release-crate` OR chain |

A README line without the annotation stays at the old version while the release reports success. A version named in `[dev-dependencies]` is never rewritten, so it falls behind and the lockfile sync cannot resolve it. A missed workflow seat still writes the right version into every file; only the lockfile sync skips the crate.

### Placeholder Crate

The placeholder reserves the name, and Trusted Publishing is configured against it in crates.io's settings rather than in this repository, so it comes first.

1. Create it outside the repository, so no workspace manifest picks it up.
2. Give it version `0.0.0`, a description of the real crate, `license`, and `repository`.
3. Give it a `src/lib.rs` holding only a doc comment.
4. Publish it by hand.

The real version then publishes through the normal release. Publishing skips only a version already on crates.io, so the placeholder never blocks the real one.

### Trusted Publishing

Trusted Publishing matches the workflow that was triggered, never one it calls.

| Path | Triggered workflow |
|---|---|
| automated release | `release-please.yml`, calling `release-crate.yml` |
| `on: release` or manual dispatch | `release-crate.yml` |

A config naming only one of the two authenticates on only one path, so it names this repository and both workflows.
