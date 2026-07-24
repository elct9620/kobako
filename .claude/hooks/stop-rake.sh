#!/bin/bash
# Stop: the full default gate (compile + test + rubocop + steep + gate),
# deferred while a benchmark is measuring so its multi-core spike does not
# contend for cores and skew the run.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:?}"

if "$root/.claude/hooks/bench-guard.sh"; then exit 0; fi

cd "$root" && bundle exec rake >&2 || exit 2
