# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/report"

# Unit coverage for the shared static-analysis output template. Each
# report task routes its sign-off through one of a fixed set of output
# types, so the family reads alike and every report states its own scope.
class KobakoReportTest < Minitest::Test
  Report = KobakoReport

  FRAMED_TABLE = [
    "+-------+-------+-----+",
    "| Name  | Files | LOC |",
    "+-------+-------+-----+",
    "| gem   |     3 | 120 |",
    "| ext   |     1 |  40 |",
    "+-------+-------+-----+",
    "| Total |     4 | 160 |",
    "+-------+-------+-----+"
  ].freeze

  def test_table_frames_an_aligned_grid_with_a_total
    lines = Report.table(
      header: %w[Name Files LOC],
      rows: [%w[gem 3 120], %w[ext 1 40]],
      total: %w[Total 4 160]
    )

    assert_equal FRAMED_TABLE, lines,
                 "rows through table must render the rails-stats framed grid: left-justified " \
                 "first column, right-justified rest, header and total ruled off"
  end

  def test_banner_carries_the_reads_as_self_description_at_the_top
    lines = Report.banner("coverage:ruby", reads_as: "Ruby lib/ lines only; Rust not measured")

    assert(lines.any? { |line| line.include?("coverage:ruby") },
           "a signal banner through the template must name the report at the top")
    assert(lines.any? { |line| line.include?("Rust not measured") },
           "a signal banner must render its reads_as scope so the number is read correctly")
  end

  def test_gate_returns_the_ok_verdict_when_clean
    assert_equal "anchors: OK — all unique", Report.gate(name: "anchors", ok_summary: "all unique"),
                 "a clean gate through the template must sign off with the shared OK verdict"
  end

  def test_gate_aborts_naming_the_count_when_violations_exist
    _out, err = capture_io do
      error = assert_raises(SystemExit) do
        Report.gate(name: "anchors", ok_summary: "all unique", violations: ["B-01 dup"], noun: "violation")
      end
      refute error.success?, "a gate with violations through the template must abort non-zero"
    end

    assert_match(/anchors: 1 violation\(s\)/, err,
                 "an aborting gate through the template must name the count with its noun")
    assert_includes err, "B-01 dup",
                    "an aborting gate through the template must list the offending lines beneath the count"
  end

  # A gate whose violations are symptoms of a procedure worth reading whole
  # names where that procedure is written, below the lines it found.
  def test_gate_trails_its_hint_beneath_the_violations
    _out, err = capture_io do
      assert_raises(SystemExit) do
        Report.gate(name: "release:wiring", ok_summary: "wired", violations: ["a seat"],
                    hint: "see docs/releasing.md")
      end
    end

    assert_match(%r{a seat\n\nsee docs/releasing\.md\n?\z}, err,
                 "a gate given a hint must trail it beneath the violations, separated by a blank line")
  end

  def test_gate_without_a_hint_ends_at_the_violations
    _out, err = capture_io do
      assert_raises(SystemExit) do
        Report.gate(name: "anchors", ok_summary: "all unique", violations: ["B-01 dup"])
      end
    end

    assert_match(/B-01 dup\n?\z/, err,
                 "a gate given no hint must end at its last violation, adding no trailing blank line")
  end

  def test_list_groups_headings_over_indented_items
    lines = Report.list([["thin:", ["  B-05 test_a", "  B-06 test_b"]], ["most cited:", ["  B-41 17"]]])

    assert_equal ["thin:", "  B-05 test_a", "  B-06 test_b", "most cited:", "  B-41 17"], lines,
                 "a list report through the template must render each heading over its items"
  end
end
