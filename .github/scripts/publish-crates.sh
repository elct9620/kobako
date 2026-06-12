#!/bin/bash
# Publish the guest crates and kobako-baker to crates.io, or rehearse
# with `--dry-run`. Runs from the `wasm/` sub-workspace directory.
#
# Dependency order: kobako depends on kobako-core, so it goes last —
# `cargo publish` waits for each crate to land in the index before
# returning. kobako-io and kobako-regexp depend only on the
# already-published beni, so their order is free.
#
# The already-published check makes a re-run after a partial failure
# resume instead of dying on "version already uploaded".
#
# Rehearsal caveat: after a release PR bumps the linked versions but
# before kobako-core publishes, the kobako dry-run fails — its
# kobako-core requirement resolves against the registry, not the
# workspace path.
set -euo pipefail

dry_run=false
[ "${1:-}" = "--dry-run" ] && dry_run=true

crate_version() {
  cargo metadata --no-deps --format-version 1 "${@:2}" \
    | jq -r ".packages[] | select(.name == \"$1\") | .version"
}

already_published() {
  curl -fsSL -A "kobako-release (github.com/elct9620/kobako)" \
    "https://crates.io/api/v1/crates/$1/$2" >/dev/null 2>&1
}

for crate in kobako-core kobako-io kobako-regexp kobako; do
  if $dry_run; then
    cargo publish -p "$crate" --dry-run
    continue
  fi
  version="$(crate_version "$crate")"
  if already_published "$crate" "$version"; then
    echo "$crate $version already on crates.io; skipping"
    continue
  fi
  cargo publish -p "$crate"
done

# kobako-baker lives beside the workspace members but is a standalone
# host-side crate — publish via its own manifest.
if $dry_run; then
  cargo publish --manifest-path kobako-baker/Cargo.toml --dry-run
else
  version="$(crate_version kobako-baker --manifest-path kobako-baker/Cargo.toml)"
  if already_published kobako-baker "$version"; then
    echo "kobako-baker $version already on crates.io; skipping"
  else
    cargo publish --manifest-path kobako-baker/Cargo.toml
  fi
fi
