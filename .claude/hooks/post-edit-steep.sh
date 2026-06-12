#!/bin/bash
# PostToolUse(Edit|Write): whole-project Steep type check; an edit that
# breaks the RBS signatures blocks. The sig/ tree mirrors lib/ 1:1, so a
# .rb edit without a matching .rbs update fails here.
set -euo pipefail

file=$(jq -r '.tool_input.file_path | select(test("\\.(rb|rbs)$"))')
[ -n "$file" ] || exit 0

cd "${CLAUDE_PROJECT_DIR:?}"
if ! bundle exec steep check >&2; then
  echo "[steep] type check failed after editing $file" >&2
  exit 2
fi
