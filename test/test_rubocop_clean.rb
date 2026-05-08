# frozen_string_literal: true

require "English"
require "minitest/autorun"

# Durable CI guard: enforces a zero-offense rubocop baseline so any future
# commit that introduces a new offense fails this test. Skips when rubocop is
# not installed (e.g. when the gem is consumed as a dependency rather than
# developed in this checkout). SPEC.md "Implementation Standards" treats lint
# cleanliness as a per-cycle invariant; this test makes that invariant
# executable rather than relying on developer discipline.
class TestRubocopClean < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)

  def test_rubocop_reports_no_offenses
    skip "set KOBAKO_LINT=1 to run rubocop in the test suite" unless ENV["KOBAKO_LINT"] == "1"

    rubocop_available = system("bundle", "exec", "rubocop", "--version",
                               out: File::NULL, err: File::NULL,
                               chdir: PROJECT_ROOT)
    skip "rubocop not available in this environment" unless rubocop_available

    output = IO.popen(["bundle", "exec", "rubocop", "--format", "simple"],
                      chdir: PROJECT_ROOT, err: %i[child out], &:read)
    status = $CHILD_STATUS

    assert_predicate status, :success?,
                     "rubocop reported offenses; the baseline must stay clean.\n" \
                     "Run `bundle exec rubocop -A` and commit fixes, or justify\n" \
                     "the new offense via .rubocop.yml with a rationale comment.\n\n" \
                     "#{output}"
  end
end
