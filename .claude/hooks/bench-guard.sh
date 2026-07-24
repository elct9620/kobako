#!/bin/bash
# Stop-hook guard. While a benchmark measures, tmp/.bench.lock (written by
# Kobako::Bench::Lock) names the live bench process as two lines: its pid
# and its `ps` start time. Exit 0 so the caller defers its CPU-heavy gate
# rather than contend for cores and skew the numbers. A lock left by a
# killed run — a dead pid, or a pid now reused by an unrelated process — is
# stale: it is removed here and exit 1 lets the caller run its gate.
set -uo pipefail

root="${CLAUDE_PROJECT_DIR:?}"
lock="$root/tmp/.bench.lock"

[ -f "$lock" ] || exit 1

pid="$(sed -n 1p "$lock")"
start="$(sed -n 2p "$lock")"

# Trim surrounding whitespace so the comparison matches the Ruby side's
# stripped value; `ps -o lstart=` pads its output.
trim() {
  local s="$1"
  s="${s#"${s%%[![:space:]]*}"}"
  printf '%s' "${s%"${s##*[![:space:]]}"}"
}
now="$(trim "$(ps -p "$pid" -o lstart= 2>/dev/null)")"

if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null && [ -n "$now" ] && [ "$now" = "$start" ]; then
  echo "[bench-guard] benchmark running (pid $pid) — deferring the Stop gate" >&2
  exit 0
fi

rm -f "$lock"
exit 1
