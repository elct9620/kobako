#!/bin/bash
# PostToolUse(Edit|Write): whole-project Steep type check; an edit that
# breaks the RBS signatures blocks. The sig/ tree mirrors lib/ 1:1, so a
# .rb edit without a matching .rbs update fails here. Only lib/ and sig/
# are what the Steepfile reads, so an edit anywhere else cannot change the
# answer and runs nothing.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:?}"
file=$(jq -r '.tool_input.file_path | select(test("\\.(rb|rbs)$"))')
case "$file" in
  "$root"/lib/* | "$root"/sig/*) ;;
  *) exit 0 ;;
esac

cd "$root"
if ! bundle exec steep check >&2; then
  echo "[steep] type check failed after editing $file" >&2
  exit 2
fi
