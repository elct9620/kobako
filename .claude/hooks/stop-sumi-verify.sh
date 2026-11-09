#!/bin/bash
# Stop: hold the source to the specification — the vocabulary it is written
# in, the behaviors its tests claim, and the interfaces it registers.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:?}"

# Nothing to check with when the tool is absent, which a fresh clone is.
command -v sumi >/dev/null || {
  echo "[sumi] not installed — skipping the specification check" >&2
  exit 0
}

source "$root/.claude/hooks/stop-gate.sh"
gate_start sumi

cd "$root"
sumi verify >"$GATE_LOG" 2>&1 || gate_fail sumi <"$GATE_LOG"
