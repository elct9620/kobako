# frozen_string_literal: true

require "test_helper"
require "json"

require_relative "../../tasks/support/rust_coverage"

# Unit coverage for the reader that turns `cargo llvm-cov --json` into the
# concise coverage:crates / coverage:wasm table: only files below full
# line coverage, worst first, over the workspace total.
class KobakoRustCoverageTest < Minitest::Test
  Reader = KobakoRustCoverage
  ROOT = "/repo"

  def export(files, totals)
    { "data" => [{ "files" => files, "totals" => { "lines" => totals } }] }.to_json
  end

  def file(name, covered, count, percent)
    { "filename" => "#{ROOT}/#{name}",
      "summary" => { "lines" => { "covered" => covered, "count" => count, "percent" => percent } } }
  end

  def test_table_drops_full_coverage_files_and_orders_worst_first
    json = export(
      [file("crates/mid.rs", 5, 10, 50.0), file("crates/full.rs", 10, 10, 100.0), file("crates/none.rs", 0, 10, 0.0)],
      { "covered" => 15, "count" => 30, "percent" => 50.0 }
    )
    lines = Reader.table(json, root: ROOT)

    refute(lines.any? { |line| line.include?("full.rs") },
           "a file at full line coverage through table must be dropped so only paths needing attention show")
    none_at = lines.index { |line| line.include?("none.rs") }
    mid_at = lines.index { |line| line.include?("mid.rs") }
    assert(none_at < mid_at, "files through table must be ordered worst-covered first")
  end

  def test_table_relativizes_paths_and_carries_the_total
    json = export([file("crates/a.rs", 1, 4, 25.0)], { "covered" => 1, "count" => 4, "percent" => 25.0 })
    lines = Reader.table(json, root: ROOT)

    assert(lines.any? { |line| line.include?("crates/a.rs") && line.include?("1/4") && line.include?("25.0%") },
           "a file row through table must relativize the path and show covered/count and percent")
    assert(lines.any? { |line| line.include?("TOTAL") && line.include?("1/4") },
           "the workspace total through table must render as the framed total row")
  end
end
