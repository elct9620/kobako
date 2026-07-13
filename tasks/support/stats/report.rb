# frozen_string_literal: true

require_relative "../report"

# The report-rendering half of +KobakoStats+ (+tasks/support/stats.rb+
# holds the measurement half): category rows in, an aligned rails-stats
# -style table out, with the code-to-test ratio summary line.
module KobakoStats
  module_function

  HEADER = %w[Name Files Lines LOC Comments].freeze

  ZERO_ROW = { files: 0, blank: 0, comment: 0, code: 0 }.freeze

  # Render category rows as an aligned table with a Total row and the
  # rails-stats-style code-to-test summary line. +rust_test_loc+ is the
  # code-LOC of the Rust inline +#[cfg(test)]+ tails, moved from the code
  # side to the test side so the ratio counts them as tests.
  def table(rows, rust_test_loc: 0)
    [grid(rows), ratio_line(rows, rust_test_loc: rust_test_loc), ""].join("\n")
  end

  # The framed table with its Total row, without the ratio summary — the
  # tier report adds the ratio through +table+, and the per-module detail
  # frames one module's languages before its own footer. The framing
  # itself is the shared +:table+ type; this only supplies the rows.
  def grid(rows)
    body = rows.map { |row| cells(row) }
    KobakoReport.table(header: HEADER, rows: body, total: cells(total(rows))).join("\n")
  end

  # A single module's footer: the same code-to-test line every stats view
  # ends with, counting a crate's Rust inline +#[cfg(test)]+ as its tests.
  # The gem's tests live in test/, so it is noted rather than shown as a
  # bare 1:0.0.
  def module_footer(impl:, test:)
    return "  Code LOC: #{impl}    Test LOC: —    (Ruby suite lives in test/)" if test.nil?

    ratio_summary(impl, test)
  end

  def cells(row)
    lines = row[:blank] + row[:comment] + row[:code]
    [row[:name], row[:files].to_s, lines.to_s, row[:code].to_s, row[:comment].to_s]
  end

  def total(rows)
    ZERO_ROW.to_h { |key, _| [key, rows.sum { |row| row[key] }] }.merge(name: "Total")
  end

  # Only +:code+ and +:test+ rows enter the ratio — signatures, docs, and
  # tooling are reported but not weighed against implementation.
  # +rust_test_loc+ crosses over from code to test: cloc folds a crate's
  # inline +#[cfg(test)]+ tests into its code count because they share a
  # file, so without it the ratio understates a Rust-heavy repo's tests.
  def ratio_line(rows, rust_test_loc: 0)
    summary = ratio_summary(kind_loc(rows, :code) - rust_test_loc, kind_loc(rows, :test) + rust_test_loc)
    return summary if rust_test_loc.zero?

    "#{summary}\n  (Rust inline #[cfg(test)]: #{rust_test_loc} LOC counted as test)"
  end

  # The rails-stats code-to-test footer every stats view ends with — the
  # tier table, the per-module detail, and the module footer all render
  # it, so one template governs the ratio wherever it appears.
  def ratio_summary(code, test)
    ratio = code.zero? ? 0.0 : test.fdiv(code).round(1)
    "  Code LOC: #{code}    Test LOC: #{test}    Code to Test Ratio: 1:#{ratio}"
  end

  def kind_loc(rows, kind)
    rows.sum { |row| row[:kind] == kind ? row[:code] : 0 }
  end
end
