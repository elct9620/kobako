#!/bin/bash
# Stop: the full default task (`rake -P` lists it).
#
# The log names each task and each test as it starts, so a run that stalls
# names where. A warm run takes under two minutes; one past LIMIT is
# stuck, so each of its processes is sampled before it is killed, and the
# hook fails naming the last task and test the run reached.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:?}"

source "$root/.claude/hooks/stop-gate.sh"
gate_start rake

LIMIT=600

# Each minitest failure and cargo's failures section; the log's end when
# the run failed elsewhere.
failures() {
  local found
  found="$(awk '
    /^ *[0-9]+\) (Failure|Error):/ { mode = "minitest" }
    /^failures:$/                  { mode = "cargo" }
    mode == "minitest" && /^$/     { mode = ""; print; next }
    mode != ""                     { print }
    /^test result:/                { mode = "" }
  ' "$GATE_LOG")"
  if [ -n "$found" ]; then echo "$found"; else tail -n "$SUMMARY_LINES" "$GATE_LOG"; fi
}

# Where the stuck run stood, its process group sampled and then killed.
stuck() {
  local samples="$STOP_LOG_DIR/rake-samples" pid
  rm -rf "$samples" && mkdir -p "$samples"
  ps -o pid,etime,command -g "$run" >"$samples/ps.txt" 2>&1 || true
  for pid in $(pgrep -g "$run"); do sample "$pid" 1 -file "$samples/$pid.txt" >/dev/null 2>&1 || true; done
  kill -KILL -- "-$run" 2>/dev/null || true
  echo "ran past ${LIMIT}s"
  echo "last task: $(grep -E '\*\* Execute ' "$GATE_LOG" | tail -n 1)"
  echo "last test: $(grep -E '#test_| has been running for over' "$GATE_LOG" | tail -n 1)"
  echo "samples: $samples"
}

cd "$root"

# The tree a run judges: HEAD, every tracked change, and each untracked file
# git would not ignore. A stop finding the tree a green run already judged
# runs nothing, so a stop that changed nothing costs no second run.
tree_key() {
  {
    git rev-parse HEAD
    git diff HEAD
    git ls-files --others --exclude-standard -z | xargs -0 shasum 2>/dev/null
  } | shasum | cut -d' ' -f1
}
green="$STOP_LOG_DIR/rake.green"
key="$(tree_key)"
if [ -f "$green" ] && [ "$(cat "$green")" = "$key" ]; then
  echo "[rake] this tree passed already — skipping" >&2
  exit 0
fi

# A process group of its own, so the watch below reaches every process the
# run spawns.
set -m
bundle exec rake --trace A=-v >>"$GATE_LOG" 2>&1 &
run=$!
set +m
started=$SECONDS
while kill -0 "$run" 2>/dev/null; do
  if ((SECONDS - started >= LIMIT)); then gate_fail rake < <(stuck); fi
  sleep 1
done
wait "$run" || gate_fail rake < <(failures)
# Only a tree no edit touched during the run counts as the one it judged.
if [ "$(tree_key)" = "$key" ]; then echo "$key" >"$green"; fi
