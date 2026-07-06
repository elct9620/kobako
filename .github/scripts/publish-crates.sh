#!/bin/bash
# Publish the guest crates, kobako-baker, and the crates/ host crates
# to crates.io, or rehearse with `--dry-run`. Runs from the `wasm/`
# sub-workspace directory.
#
# Dependency order: kobako-codec is everyone's wire tier, so it goes
# first (from the crates/ workspace); kobako-mruby depends on
# kobako-core, so it goes last in the guest loop — `cargo publish`
# waits for each crate to land in the index before returning.
# kobako-io, kobako-json, and kobako-regexp depend only on the
# already-published beni, so their order is free. On the host side
# kobako-wasmtime depends on kobako-runtime, so runtime goes first.
#
# The already-published check makes a re-run after a partial failure
# resume instead of dying on "version already uploaded".
#
# Rehearsal caveat: after a release PR bumps the linked versions but
# before the dependency publishes, a dependent's dry-run fails — the
# kobako-codec requirement of kobako-core and kobako-mruby, the
# kobako-core requirement of kobako-mruby, and the kobako-runtime
# requirement of kobako-wasmtime, all resolve against the registry,
# not the workspace path.
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

# The shared wire tier lives in the crates/ workspace but every guest
# crate below depends on it — publish it before anything else.
if $dry_run; then
  cargo publish --manifest-path ../crates/Cargo.toml -p kobako-codec --dry-run
else
  version="$(crate_version kobako-codec --manifest-path ../crates/Cargo.toml)"
  if already_published kobako-codec "$version"; then
    echo "kobako-codec $version already on crates.io; skipping"
  else
    cargo publish --manifest-path ../crates/Cargo.toml -p kobako-codec
  fi
fi

for crate in kobako-core kobako-io kobako-json kobako-regexp kobako-mruby; do
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

# Host crates from the crates/ workspace (the Ruby ext's path
# dependencies, published for non-Ruby hosts).
for crate in kobako-runtime kobako-wasmtime; do
  if $dry_run; then
    cargo publish --manifest-path ../crates/Cargo.toml -p "$crate" --dry-run
    continue
  fi
  version="$(crate_version "$crate" --manifest-path ../crates/Cargo.toml)"
  if already_published "$crate" "$version"; then
    echo "$crate $version already on crates.io; skipping"
    continue
  fi
  cargo publish --manifest-path ../crates/Cargo.toml -p "$crate"
done
