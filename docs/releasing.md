# Releasing

Two independent release tracks run off one `release-please` config:

| Track | Package(s) | Tag | Registry |
|-------|-----------|-----|----------|
| Gem | `.` (component `kobako`) | `vX.Y.Z` | RubyGems |
| Linked crate group | `wasm/*` + `crates/*` (10 components, versions locked together) | `<component>-vX.Y.Z` | crates.io |

`release-please` reads the conventional-commit history since each track's last release and opens a release PR. **Which track a commit drives is decided by the paths it touches**, and the version bump by its type. Merging the release PR is the only irreversible step (RubyGems has no repush; crates.io is yank-only).

## Which track a commit triggers

The gem `.` package is greedy (any root change) but carries `exclude-paths: ["wasm", "crates"]`; the crate packages claim their own subtrees.

| A commit that touches… | Triggers |
|------------------------|----------|
| only root files (`lib/ ext/ sig/ test/ docs/ SPEC.md README.md examples/ …`) | Gem |
| only `wasm/*` or `crates/*` | Linked crate group (any one component → linked-versions syncs all 10) |
| both root **and** `wasm/`/`crates/` | **Both** — avoid unless a coordinated dual release is intended |

`exclude-paths` must stay **symmetric**: both `wasm/` and `crates/` have a workspace-root `Cargo.lock`/`Cargo.toml` that no member package claims, so both must be excluded from `.` or crate-only work leaks into the gem.

## Version bump per commit type

Standard semver, the mapping once the project is stable (≥ 1.0):

| Type | Bump |
|------|------|
| `feat:` | minor |
| `fix:` | patch |
| `feat!` / `fix!` / `BREAKING CHANGE:` footer | major |
| `refactor` `docs` `test` `chore` `build` `ci` `perf` `style` | none — non-releasing |

A `refactor`/`docs`/`test` commit never releases: a change shipped only under those types is invisible to `release-please`.

**Temporary pre-1.0 override.** While the project is in 0.x the config sets `bump-minor-pre-major: true` **globally** (top-level, so both the gem and the crate group inherit it), which remaps a breaking change to a **minor** bump so a `!` stays inside 0.x instead of cutting `1.0.0`. This is a stabilization-period device — **remove it uniformly when the project adopts 1.0**, after which a breaking change bumps the major per the table above. Without the flag a `!` on a 0.x version jumps straight to `1.0.0`.

## Cutting a release per situation

| Situation | Approach |
|-----------|----------|
| Package has a `feat`/`fix` on its paths since last release | Nothing extra — `release-please` opens the PR automatically. |
| Catch-up: real change landed only as `refactor`/`docs`/`test` (e.g. behavior shipped inside the bundled `kobako.wasm`) | Give it a trigger. **Preferred:** land a genuine `feat`/`fix` touching that track's paths (correcting a stale README example is a real `fix`). **Fallback (no natural feat/fix exists):** a `Release-As: X.Y.Z` footer — see rules below. |
| Both tracks must publish together (gem bundles the guest; a gem on the old guest mispairs with new crates) | Put **both** triggers in the same scan window, each path-scoped to its own track. `release-please` opens one combined PR; a single merge publishes both. |
| A change is breaking | Put the `!` / `BREAKING CHANGE:` on the track whose **public surface** actually broke — the gem for a guest-Ruby idiom (guest scripts are gem-user-facing), a crate for its Rust API. A change can break one track and not the other: the `Kobako::Member` → `Kobako::Proxy` guest change broke gem-user scripts but left the crate Rust API intact. |

## `Release-As` rules (the footgun cluster)

Use `Release-As` only when a track has no natural `feat`/`fix` to release.

| Rule | Why |
|------|-----|
| The `Release-As: X.Y.Z` **commit footer** triggers; the `release-as` **config key alone never does**. | The config only overrides a version once a release is already happening. |
| A `Release-As` footer is **global across every component releasing in that window**. | Never have two different `Release-As` footers in one window — they collide. |
| **Never** `git commit --allow-empty` with `Release-As`. | An empty commit touches no path, so `exclude-paths` cannot scope the footer — it forces the version on every component, including the gem. |
| Keep a `Release-As` commit's file changes within **one track's paths** (wasm-only for crates, root-only for the gem). | `exclude-paths` then scopes the footer to that track; the other track ignores the commit. Touching a version snippet in each of the track's READMEs is the canonical vehicle. |

## Before merging a release PR

| Check | Guards against |
|-------|----------------|
| Read `.release-please-manifest.json` in the PR diff | A track dragged to the wrong version — e.g. gem jumping to `1.0.0`, or one track inheriting the other's `Release-As`. |
| Confirm each track's version matches intent | Silent bump errors before the irreversible publish. |
| `examples/` were **not** changed as the release vehicle | `examples/` pin released versions (`gem "kobako", "~> 0.X"`); switching them to a new idiom before that version publishes breaks them against the pinned release. Update `examples/` **after** the release, then bump their pins. |
