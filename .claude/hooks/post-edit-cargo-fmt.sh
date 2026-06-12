#!/bin/bash
# PostToolUse(Edit|Write): format the edited Rust file with the rustfmt
# config of its owning crate (nearest Cargo.toml up the tree).
set -euo pipefail

file=$(jq -r '.tool_input.file_path | select(endswith(".rs"))')
[ -n "$file" ] || exit 0

dir=$(dirname "$file")
while [ "$dir" != "/" ] && [ ! -f "$dir/Cargo.toml" ]; do
  dir=$(dirname "$dir")
done
[ -f "$dir/Cargo.toml" ] || exit 0

if ! cargo fmt --manifest-path "$dir/Cargo.toml" -- "$file"; then
  echo "[cargo-fmt] failed to format $file" >&2
  exit 2
fi
