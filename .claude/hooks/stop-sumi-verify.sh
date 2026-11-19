#!/bin/bash
# Stop: hold the source to the specification — the vocabulary it is written
# in, the behaviors its tests claim, and the interfaces it registers — and
# the specification to its form, as CI does, since the edit hook reaches
# only what an Edit or a Write changed.
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
{ sumi verify && sumi fmt --check; } >"$GATE_LOG" 2>&1 || gate_fail sumi <"$GATE_LOG"
