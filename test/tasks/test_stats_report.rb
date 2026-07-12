# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/stats"

# Unit coverage for the report-rendering half of +KobakoStats+
# (+tasks/support/stats/report.rb+): the tier table with its ratio line,
# the per-module roll-up with its Impl/Test split, and the framing they
# share. Fixtures are hand-built category rows so each test states only
# the rendering rule it is about; no cloc is invoked.
class KobakoStatsReportTest < Minitest::Test
  Stats = KobakoStats

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

  def test_ratio_line_reclassifies_rust_inline_test_loc_from_code_to_test
    table = Stats.table([ruby_row, test_row], rust_test_loc: 249)

    assert_includes table, "Code LOC: 1000    Test LOC: 5808    Code to Test Ratio: 1:5.8",
                    "Rust inline #[cfg(test)] LOC through table must move from the code side to the " \
                    "test side so the ratio counts inline tests as tests"
    assert_includes table, "(Rust inline #[cfg(test)]: 249 LOC counted as test)",
                    "the reclassified Rust inline-test LOC through table must be noted so the ratio's " \
                    "code figure stays reconcilable with the code-tier rows"
  end

  def test_grid_frames_rows_and_total_without_the_ratio_line
    grid = Stats.grid([ruby_row, test_row])

    assert_match(/\| Total\s*\|\s*141 \|/, grid,
                 "the per-module grid must frame a Total row like the tier table")
    refute_includes grid, "Code to Test Ratio",
                    "the per-module grid must omit the code-to-test ratio, which weighs tiers not modules"
  end

  def test_module_roll_up_splits_impl_and_test_with_a_gem_dash
    out = module_roll_up_sample

    assert_match(/\| Module\s+\| Impl\s+\| Test\s+\| Test%\s+\| Comment \|/, out,
                 "the roll-up header must carry the Impl/Test/Test%/Comment columns")
    assert_match(/\| kobako \(gem\)\s+\|\s+2299 \|\s+— \|\s+— \|\s+1921 \|/, out,
                 "a gem row must dash Test and Test% since its Ruby tests live in test/")
    assert_match(/\| kobako-codec\s+\|\s+857 \|\s+1090 \|\s+56% \|\s+395 \|/, out,
                 "a crate row must show inline test LOC and its Test% share")
    assert_match(/\| Total\s+\|\s+3156 \|\s+1090 \|\s+— \|\s+2316 \|/, out,
                 "the Total row must sum Impl/Test/Comment and dash the aggregate Test%")
  end

  def test_module_roll_up_prints_the_interpretation_legend
    assert_includes module_roll_up_sample, "guest crates",
                    "module_roll_up must print the legend explaining why guest crates read low on inline Test%"
  end

  def test_module_summary_lines_up_impl_and_test_share
    assert_equal "  Impl: 857    Inline test: 1090    Test%: 56%",
                 Stats.module_summary(impl: 857, test: 1090),
                 "a crate's module_summary must line up Impl, inline Test LOC, and Test%"
  end

  def test_module_summary_points_the_gem_at_its_external_suite
    assert_includes Stats.module_summary(impl: 2299, test: nil), "test/",
                    "the gem's module_summary must point at its external Ruby suite instead of a Test%"
  end

  private

  def module_roll_up_sample
    Stats.module_roll_up(
      [{ name: "kobako (gem)", impl: 2299, test: nil, comment: 1921 },
       { name: "kobako-codec", impl: 857, test: 1090, comment: 395 }]
    )
  end

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
