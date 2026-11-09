#!/bin/bash
# Sourced by each Stop hook. A gate's whole output goes to a log of its own,
# kept outside the repository for the latest run only, and a failure hands
# back at most SUMMARY_LINES of what failed plus where the log is, so a
# broken gate cannot flood the conversation.

STOP_LOG_DIR="${TMPDIR:-/tmp}"
STOP_LOG_DIR="${STOP_LOG_DIR%/}/kobako-stop-hook"
SUMMARY_LINES=80

# Start this hook's gate, emptying the log an earlier run left. A gate is
# deferred while a benchmark measures, as its CPU spike would contend for
# cores and skew the numbers.
gate_start() {
  if "$CLAUDE_PROJECT_DIR/.claude/hooks/bench-guard.sh"; then exit 0; fi
  mkdir -p "$STOP_LOG_DIR"
  GATE_LOG="$STOP_LOG_DIR/$1.log"
  : >"$GATE_LOG"
}

# Fail the hook as `label`, handing back the summary read from stdin, cut to
# SUMMARY_LINES, and the path of the whole output.
gate_fail() {
  local label="$1"
  {
    echo "[$label] failed"
    head -n "$SUMMARY_LINES"
    echo "[$label] full output: $GATE_LOG"
  } >&2
  exit 2
}
