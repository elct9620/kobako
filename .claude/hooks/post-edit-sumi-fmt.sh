#!/bin/bash
# PostToolUse(Edit|Write): write the edited specification file the way a
# reference line is. sumi fmt reads its root from .sumi.json and rewrites
# the whole specification, so the edited path decides whether to run at
# all rather than what to run on.
set -euo pipefail

file=$(jq -r '.tool_input.file_path | select(test("docs/spec/.*\\.md$"))')
[ -n "$file" ] || exit 0

cd "${CLAUDE_PROJECT_DIR:?}"

# Nothing to write with when the tool is absent, which a fresh clone is.
command -v sumi >/dev/null || exit 0

if ! sumi fmt >&2; then
  echo "[sumi-fmt] failed to write the specification after editing $file" >&2
  exit 2
fi
