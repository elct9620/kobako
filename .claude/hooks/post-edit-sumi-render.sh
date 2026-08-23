#!/bin/bash
# PostToolUse(Edit|Write): rewrite the specification's documents from the
# declaration just edited, so the page a reader opens is never behind it.
# Nothing else compares the two — `sumi render` only ever writes.
set -euo pipefail

file=$(jq -r '.tool_input.file_path | select(test("/\\.spec/") or endswith("/.sumi.json"))')
[ -n "$file" ] || exit 0

cd "${CLAUDE_PROJECT_DIR:?}"
command -v sumi >/dev/null || exit 0

if ! sumi render >&2; then
  echo "[sumi-render] $file left the specification unreadable" >&2
  exit 2
fi
