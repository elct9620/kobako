#!/bin/bash
# Run the release-tooling unit tests (every *_test.rb suite under
# tasks/support/, discovered by glob) and report each suite to the
# GitHub job summary as a Markdown table.
#
# Non-blocking by design: the gates themselves block in `rake default`,
# so a failed suite here raises a warning annotation and is recorded in
# the summary, but the step still exits 0 and never reddens the build.
set -uo pipefail

summary="${GITHUB_STEP_SUMMARY:-/dev/stdout}"
failed=0
details=""

{
  echo "## Release-tooling unit tests"
  echo
  echo "| Suite | Runs | Assertions | Failures | Errors | Skips | Status |"
  echo "| --- | --: | --: | --: | --: | --: | :-: |"
} >> "$summary"

for file in $(find tasks/support -name '*_test.rb' | sort); do
  suite="${file#tasks/support/}"
  out=$(bundle exec ruby "$file" 2>&1)
  line=$(echo "$out" | grep -E '[0-9]+ runs,' | tail -1)

  if [[ "$line" =~ ([0-9]+)\ runs,\ ([0-9]+)\ assertions,\ ([0-9]+)\ failures,\ ([0-9]+)\ errors,\ ([0-9]+)\ skips ]]; then
    runs="${BASH_REMATCH[1]}"
    assertions="${BASH_REMATCH[2]}"
    failures="${BASH_REMATCH[3]}"
    errors="${BASH_REMATCH[4]}"
    skips="${BASH_REMATCH[5]}"
    if [ "$failures" -eq 0 ] && [ "$errors" -eq 0 ]; then
      status="✅"
    else
      status="⚠️"
      failed=1
    fi
    echo "| \`$suite\` | $runs | $assertions | $failures | $errors | $skips | $status |" >> "$summary"
  else
    # No minitest summary line: the suite crashed before reporting.
    status="💥"
    failed=1
    echo "| \`$suite\` | — | — | — | — | — | $status |" >> "$summary"
    details+=$'\n'"<details><summary>\`$suite\` output</summary>"$'\n\n```\n'"$out"$'\n```\n\n</details>\n'
  fi
done

if [ -n "$details" ]; then
  { echo; echo "$details"; } >> "$summary"
fi

if [ "$failed" -ne 0 ]; then
  echo "::warning title=Release-tooling tests::a tasks/support unit suite failed (non-blocking) — see job summary"
fi

exit 0
