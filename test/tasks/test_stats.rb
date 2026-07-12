# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/stats"

# Unit coverage for the measurement half of +KobakoStats+
# (+tasks/support/stats/report.rb+ has its own rendering suite): the pure
# pieces — cloc JSON aggregation and the tracked-file exclusion rule — are
# exercised without invoking cloc itself, so the suite runs on machines
# where cloc is absent. Fixtures are hand-built cloc outputs.
class KobakoStatsTest < Minitest::Test
  Stats = KobakoStats

  CLOC_JSON = <<~JSON
    {
      "header": { "cloc_version": "2.08" },
      "Ruby": { "nFiles": 36, "blank": 441, "comment": 312, "code": 1249 },
      "Rust": { "nFiles": 6, "blank": 40, "comment": 95, "code": 353 },
      "SUM": { "blank": 481, "comment": 407, "code": 1602, "nFiles": 42 }
    }
  JSON

  def test_sum_aggregates_the_cloc_json_totals
    assert_equal({ files: 42, blank: 481, comment: 407, code: 1602 },
                 Stats.sum(CLOC_JSON),
                 "cloc JSON output through sum must yield the SUM section as a totals row")
  end

  def test_sum_of_empty_cloc_output_is_a_zero_row
    assert_equal({ files: 0, blank: 0, comment: 0, code: 0 },
                 Stats.sum(""),
                 "empty cloc output through sum must yield an all-zero totals row")
  end

  def test_by_language_lists_each_language_heaviest_first_without_header_or_sum
    assert_equal(
      [{ name: "Ruby", files: 36, blank: 441, comment: 312, code: 1249 },
       { name: "Rust", files: 6, blank: 40, comment: 95, code: 353 }],
      Stats.by_language(CLOC_JSON),
      "a cloc JSON report through by_language must yield one row per language, heaviest first, " \
      "dropping the header and SUM sections"
    )
  end

  def test_by_language_of_empty_cloc_output_is_no_rows
    assert_empty Stats.by_language(""),
                 "empty cloc output through by_language must yield no language rows"
  end

  def test_non_implementation_artifacts_are_excluded
    %w[Gemfile.lock crates/Cargo.lock docs/diagram.svg data/.keep
       benchmark/results/2026-07-01-abc1234.json benchmark/baseline.json].each do |path|
      assert Stats.excluded?(path),
             "generated artifact #{path} through excluded? must be excluded from the count"
    end
  end

  def test_source_files_are_not_excluded
    %w[lib/kobako.rb crates/kobako-codec/Cargo.toml docs/wire-codec.md benchmark/support/gate.rb].each do |path|
      refute Stats.excluded?(path),
             "tracked source #{path} through excluded? must be counted"
    end
  end
end
