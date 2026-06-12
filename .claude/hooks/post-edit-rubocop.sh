#!/bin/bash
# PostToolUse(Edit|Write): auto-correct the edited Ruby file; a remaining
# offense blocks the edit.
set -euo pipefail

file=$(jq -r '.tool_input.file_path | select(endswith(".rb"))')
[ -n "$file" ] || exit 0

cd "${CLAUDE_PROJECT_DIR:?}"
if ! bundle exec rubocop -A --stderr "$file"; then
  echo "[rubocop] offenses remain in $file after auto-correct" >&2
  exit 2
fi
