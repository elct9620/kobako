# frozen_string_literal: true

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
  # per-module roll-up reports code sizes side by side, and the ratio
  # weighs the code and test tiers, which no single module carries.
  def grid(rows)
    body = rows.map { |row| cells(row) }
    foot = cells(total(rows))
    widths = [HEADER, foot, *body].transpose.map { |column| column.map(&:length).max }
    framed(body, foot, widths).join("\n")
  end

  def framed(body, foot, widths)
    rule = "+#{widths.map { |width| "-" * (width + 2) }.join("+")}+"
    [rule, line(HEADER, widths), rule,
     *body.map { |row| line(row, widths) },
     rule, line(foot, widths), rule]
  end

  def cells(row)
    lines = row[:blank] + row[:comment] + row[:code]
    [row[:name], row[:files].to_s, lines.to_s, row[:code].to_s, row[:comment].to_s]
  end

  def total(rows)
    ZERO_ROW.to_h { |key, _| [key, rows.sum { |row| row[key] }] }.merge(name: "Total")
  end

  def line(values, widths)
    padded = values.each_with_index.map do |value, index|
      index.zero? ? value.ljust(widths[index]) : value.rjust(widths[index])
    end
    "| #{padded.join(" | ")} |"
  end

  # Only +:code+ and +:test+ rows enter the ratio — signatures, docs, and
  # tooling are reported but not weighed against implementation.
  # +rust_test_loc+ crosses over from code to test: cloc folds a crate's
  # inline +#[cfg(test)]+ tests into its code count because they share a
  # file, so without it the ratio understates a Rust-heavy repo's tests.
  def ratio_line(rows, rust_test_loc: 0)
    code = kind_loc(rows, :code) - rust_test_loc
    test = kind_loc(rows, :test) + rust_test_loc
    ratio = code.zero? ? 0.0 : test.fdiv(code).round(1)
    summary = "  Code LOC: #{code}    Test LOC: #{test}    Code to Test Ratio: 1:#{ratio}"
    return summary if rust_test_loc.zero?

    "#{summary}\n  (Rust inline #[cfg(test)]: #{rust_test_loc} LOC counted as test)"
  end

  def kind_loc(rows, kind)
    rows.sum { |row| row[:kind] == kind ? row[:code] : 0 }
  end
end
