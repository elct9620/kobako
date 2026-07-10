# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/coverage"

# Unit coverage for the line-coverage reporter backing +rake coverage+:
# entry collection keeps only executable +lib/kobako/+ sources sorted
# worst-first, and the report renders per-file rows, truncated
# uncovered-line pointers, and an aggregate total.
class KobakoCoverageTest < Minitest::Test
  Reporter = KobakoCoverage

  def test_collect_keeps_only_executable_lib_sources_worst_first
    result = {
      "#{Reporter::LIB_ROOT}/sandbox.rb" => [1, 0, nil, 2],
      "#{Reporter::LIB_ROOT}/errors.rb" => [1, 1],
      "#{Reporter::LIB_ROOT}/empty.rb" => [nil, nil],
      "/usr/lib/ruby/set.rb" => [1, 0]
    }

    assert_equal %w[sandbox.rb errors.rb], Reporter.collect(result).map { |entry| entry[:name] },
                 "a Coverage result through collect must keep lib/kobako/ files with executable lines, " \
                 "sorted lowest coverage first"
  end

  def test_collect_builds_the_entry_from_hit_counts
    entry = Reporter.collect({ "#{Reporter::LIB_ROOT}/sandbox.rb" => [3, 0, nil, 1, 0] }).first

    assert_equal({ name: "sandbox.rb", covered: 2, relevant: 4, pct: 50.0, uncov: [2, 5] }, entry,
                 "hit counts through collect must yield covered/relevant/pct and 1-based uncovered lines")
  end

  def test_report_lines_render_rows_totals_and_truncated_uncovered_pointers
    hits = ([0] * 9) + [1]
    lines = Reporter.report_lines({ "#{Reporter::LIB_ROOT}/sandbox.rb" => hits })

    assert_includes lines, "sandbox.rb    1/ 10   10.0%  uncovered: 1,2,3,4,5,6,7,8…",
                    "a file row through report_lines must show counts, percentage, and the first " \
                    "eight uncovered lines with an ellipsis"
    assert_includes lines, "TOTAL: 1/10 (10.0%)",
                    "entries through report_lines must aggregate into one TOTAL line"
  end

  def test_report_lines_of_an_empty_result_explain_the_empty_suite
    assert_equal ["No lib/kobako/ source files were loaded — empty suite?"], Reporter.report_lines({}),
                 "a result with no lib/kobako/ entry through report_lines must explain instead of rendering"
  end
end
