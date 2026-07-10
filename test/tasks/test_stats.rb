# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/stats"

# Unit coverage for the code-statistics helper backing +tasks/stats.rake+:
# the pure pieces — cloc JSON aggregation, the tracked-file exclusion rule,
# and the report table — are exercised without invoking cloc itself, so the
# suite runs on machines where cloc is absent. Fixtures are hand-built cloc
# outputs and category rows so each test states only the rule it is about.
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

  def test_table_renders_one_row_per_category_with_derived_total_lines
    table = Stats.table([ruby_row])

    assert_match(%r{\| Ruby API \(lib/\)\s*\|\s*36 \|\s*2002 \|\s*1249 \|\s*312 \|}, table,
                 "a category row through table must show files, blank+comment+code lines, LOC, and comments")
  end

  def test_table_total_row_aggregates_all_categories
    table = Stats.table([ruby_row, test_row])

    assert_match(/\| Total\s*\|\s*141 \|\s*9082 \|\s*6808 \|\s*1092 \|/, table,
                 "multiple category rows through table must aggregate into one Total row")
  end

  def test_table_ratio_line_reports_code_to_test_loc
    table = Stats.table([ruby_row, test_row, docs_row])

    assert_includes table, "Code LOC: 1249    Test LOC: 5559    Code to Test Ratio: 1:4.5",
                    "code and test rows through table must summarize LOC and their ratio; " \
                    "other-kind rows must stay out of the ratio"
  end

  def test_table_frame_lines_share_one_width
    table = Stats.table([ruby_row, test_row, docs_row])
    widths = table.lines.grep(/\A[+|]/).map { |line| line.chomp.length }.uniq

    assert_equal 1, widths.size,
                 "every framed line through table must align to one shared width"
  end

  def test_table_ratio_survives_zero_test_loc
    table = Stats.table([ruby_row])

    assert_includes table, "Code to Test Ratio: 1:0.0",
                    "a code-only row set through table must report a 1:0.0 ratio instead of failing"
  end

  private

  def ruby_row
    { name: "Ruby API (lib/)", kind: :code, files: 36, blank: 441, comment: 312, code: 1249 }
  end

  def test_row
    { name: "Tests (test/)", kind: :test, files: 105, blank: 741, comment: 780, code: 5559 }
  end

  def docs_row
    { name: "Docs (docs/)", kind: :other, files: 14, blank: 300, comment: 0, code: 1344 }
  end
end
