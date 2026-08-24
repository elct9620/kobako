#!/bin/bash
# Stop: hold the source to the vocabulary the specification declares —
# every line using a name the glossary rules out is reported here.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:?}"

# Defer while a benchmark is measuring, as the other Stop hooks do. The scan
# is cheap enough not to disturb one, but a measuring run leaves the source
# saying what it already said — so the answer after is the same answer.
if "$root/.claude/hooks/bench-guard.sh"; then exit 0; fi

# Nothing to check with when the tool is absent, which a fresh clone is.
command -v sumi >/dev/null || {
  echo "[sumi] not installed — skipping the vocabulary check" >&2
  exit 0
}

cd "$root" && sumi verify >&2 || exit 2
