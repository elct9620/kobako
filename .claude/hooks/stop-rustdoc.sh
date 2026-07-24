#!/bin/bash
# Stop: rustdoc gate (-D warnings, private items included) over every
# workspace; catches intra-doc links and malformed doc comments that
# clippy does not see.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:?}"

# Defer the whole gate while a benchmark is measuring — its CPU spike
# would contend for cores and skew the numbers.
if "$root/.claude/hooks/bench-guard.sh"; then exit 0; fi

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
doc crates --manifest-path "$root/crates/Cargo.toml" --workspace
doc wasm --manifest-path "$root/wasm/Cargo.toml" --workspace
doc baker --manifest-path "$root/wasm/kobako-baker/Cargo.toml"
