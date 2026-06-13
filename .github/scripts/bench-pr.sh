#!/bin/bash
# Run the SPEC regression benchmarks on the PR's base and head on the
# same runner, then diff them into the GitHub job summary. Measuring both
# revisions on one machine cancels the cross-machine variance that
# comparing against the committed baseline (blessed elsewhere) would fold
# in, so the delta reflects the PR rather than the runner. This is the
# standard same-runner relative-benchmarking pattern for shared CI.
#
# Report-only by design: a regression past the noise band surfaces as a
# warning annotation and in the summary, but the step exits 0 and never
# gates the PR.
set -uo pipefail

base_sha="${BASE_SHA:?}"
head_sha="${HEAD_SHA:?}"
work="$(mktemp -d)"

# Measure $1 (a ref) into $2 (a results JSON). Each `rake bench` writes a
# single date+sha file under benchmark/results/; clearing first leaves
# exactly that file to copy out.
bench_ref() {
  git checkout --quiet --force "$1"
  bundle install --quiet
  rm -f benchmark/results/*.json
  bundle exec rake compile wasm:build
  bundle exec rake bench
  local produced=(benchmark/results/*.json)
  cp "${produced[0]}" "$2"
}

bench_ref "$base_sha" "$work/base.json"
bench_ref "$head_sha" "$work/head.json"
git checkout --quiet --force "$head_sha"

report="$(bundle exec rake "bench:report[$work/head.json,$work/base.json]")"
echo "$report" >> "$GITHUB_STEP_SUMMARY"

if printf '%s' "$report" | grep -q '⚠️'; then
  echo "::warning title=Benchmark::PR regresses a gated benchmark past the noise band (non-blocking) — see job summary"
fi

exit 0
