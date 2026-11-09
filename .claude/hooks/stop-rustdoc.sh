#!/bin/bash
# Stop: rustdoc gate (-D warnings, private items included) over every
# workspace; catches intra-doc links and malformed doc comments that
# clippy does not see. The wasm sub-workspace builds against the host
# archive `rake beni:build` stages, found through BENI_VENDOR_DIR.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:?}"

source "$root/.claude/hooks/stop-gate.sh"
gate_start rustdoc

export RUSTDOCFLAGS="-D warnings"

doc() {
  local label="$1"
  shift
  if ! cargo doc "$@" --no-deps --document-private-items -q >>"$GATE_LOG" 2>&1; then
    gate_fail "rustdoc:$label" <"$GATE_LOG"
  fi
}

doc host --manifest-path "$root/Cargo.toml" --workspace
doc crates --manifest-path "$root/crates/Cargo.toml" --workspace
BENI_VENDOR_DIR="$root/vendor" doc wasm --manifest-path "$root/wasm/Cargo.toml" --workspace
doc baker --manifest-path "$root/wasm/kobako-baker/Cargo.toml"
