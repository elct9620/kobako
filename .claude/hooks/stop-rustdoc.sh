#!/bin/bash
# Stop: rustdoc gate (-D warnings, private items included) over every
# workspace; catches intra-doc links and malformed doc comments that
# clippy does not see.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:?}"
export RUSTDOCFLAGS="-D warnings"

doc() {
  local label="$1"
  shift
  if ! cargo doc "$@" --no-deps --document-private-items -q >&2; then
    echo "[rustdoc:$label] documentation warnings found" >&2
    exit 2
  fi
}

doc host --manifest-path "$root/Cargo.toml" --workspace
doc wasm --manifest-path "$root/wasm/Cargo.toml" --workspace
doc baker --manifest-path "$root/wasm/kobako-baker/Cargo.toml"
