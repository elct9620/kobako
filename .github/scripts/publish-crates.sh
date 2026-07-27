#!/bin/bash
# Publish the guest crates, kobako-baker, and the crates/ host crates
# to crates.io, or rehearse with `--dry-run`. Runs from the `wasm/`
# sub-workspace directory.
#
# Dependency order: kobako-transport is the fixed tier every other crate
# composes against, so it goes first (from the crates/ workspace), with
# kobako-codec beside it — `cargo publish` waits for each crate to land
# in the index before returning. kobako-mruby depends on kobako-core, so
# it goes last in the guest loop; kobako-io, kobako-json, and
# kobako-regexp depend only on the already-published beni, so their order
# is free. On the host side kobako-wasmtime depends on kobako-runtime, so
# runtime goes first, and the bare kobako SDK goes last — it depends on
# every host tier below it.
#
# The already-published check makes a re-run after a partial failure
# resume instead of dying on "version already uploaded".
#
# Rehearsal caveat: after a release PR bumps the linked versions but
# before the dependency publishes, a dependent's dry-run fails. Every
# inter-crate requirement resolves against the registry rather than the
# workspace path, so a rehearsal only succeeds ahead of the bump.
set -euo pipefail

dry_run=false
[ "${1:-}" = "--dry-run" ] && dry_run=true

crate_version() {
  cargo metadata --no-deps --format-version 1 "${@:2}" \
    | jq -r ".packages[] | select(.name == \"$1\") | .version"
}

# A version number counts as published only when a live (non-yanked)
# release occupies it. A 404 (never published) or a yanked-only slot is
# not published: the publish proceeds and cargo surfaces a genuine
# collision loudly, instead of the release skipping the crate in silence
# — the reused `kobako` name carries yanked guest-era versions. The
# `local` split keeps curl's exit status; `local body="$(…)"` would mask
# it behind local's own success.
already_published() {
  local body
  if ! body="$(curl -fsSL -A "kobako-release (github.com/elct9620/kobako)" \
    "https://crates.io/api/v1/crates/$1/$2" 2>/dev/null)"; then
    return 1
  fi
  [ "$(printf '%s' "$body" | jq -r '.version.yanked')" = "false" ]
}

# Publish one crate, or rehearse it. Trailing arguments name the manifest
# the crate resolves under; the `wasm/` workspace is the working
# directory, so its members need none.
publish_crate() {
  local name="$1"
  shift
  if $dry_run; then
    cargo publish -p "$name" "$@" --dry-run
    return
  fi
  local version
  version="$(crate_version "$name" "$@")"
  if already_published "$name" "$version"; then
    echo "$name $version already on crates.io; skipping"
    return
  fi
  cargo publish -p "$name" "$@"
}

crates_workspace=(--manifest-path ../crates/Cargo.toml)

# The fixed tier and the payload codec live in the crates/ workspace, and
# the guest crates below reach them from the registry rather than by path.
publish_crate kobako-transport "${crates_workspace[@]}"
publish_crate kobako-codec "${crates_workspace[@]}"

for crate in kobako-core kobako-io kobako-json kobako-regexp kobako-mruby; do
  publish_crate "$crate"
done

# kobako-baker lives beside the workspace members but is a standalone
# host-side crate — publish via its own manifest.
publish_crate kobako-baker --manifest-path kobako-baker/Cargo.toml

# Host crates from the crates/ workspace: the Ruby ext's path
# dependencies (published for non-Ruby hosts), then the bare kobako
# SDK over them.
for crate in kobako-runtime kobako-wasmtime kobako; do
  publish_crate "$crate" "${crates_workspace[@]}"
done
