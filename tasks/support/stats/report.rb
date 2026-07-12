# frozen_string_literal: true

# The report-rendering half of +KobakoStats+ (+tasks/support/stats.rb+
# holds the measurement half): category rows in, an aligned rails-stats
# -style table out, with the code-to-test ratio summary line.
module KobakoStats
  module_function

  HEADER = %w[Name Files Lines LOC Comments].freeze

  ZERO_ROW = { files: 0, blank: 0, comment: 0, code: 0 }.freeze

  MODULE_HEADER = %w[Module Impl Test Test% Comment].freeze

  # Reads the Impl/Test split correctly: a low inline Test% is a thin
  # crate only where inline tests are the strategy, which is not the
  # guest side or the gem.
  MODULE_LEGEND = [
    "  Impl / Test are LOC; Test counts Rust inline #[cfg(test)] only.",
    "  wasm/* guest crates are covered through test/e2e, so their inline Test% reads low by design;",
    "  the gem's Ruby suite lives in test/. Overall ratio: rake stats. Line coverage: rake coverage."
  ].freeze

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
    framed_table(HEADER, rows.map { |row| cells(row) }, cells(total(rows)))
  end

  # The per-module roll-up: one row per publishable module splitting its
  # code into implementation and inline test LOC, over the legend that
  # tells a genuinely thin crate apart from one covered through e2e.
  def module_roll_up(rows)
    body = rows.map { |row| module_cells(row) }
    foot = ["Total", rows.sum { |r| r[:impl] }.to_s, rows.sum { |r| r[:test] || 0 }.to_s,
            "—", rows.sum { |r| r[:comment] }.to_s]
    [framed_table(MODULE_HEADER, body, foot), *MODULE_LEGEND, ""].join("\n")
  end

  # A module row's cells: a nil test (the gem, whose tests live in
  # test/) dashes both Test and Test%; otherwise Test% is the inline
  # share of the module's own code.
  def module_cells(row)
    name, impl, test, comment = row.values_at(:name, :impl, :test, :comment)
    return [name, impl.to_s, "—", "—", comment.to_s] if test.nil?

    [name, impl.to_s, test.to_s, "#{test_share(impl, test)}%", comment.to_s]
  end

  def test_share(impl, test)
    (impl + test).zero? ? 0 : (100.0 * test / (impl + test)).round
  end

  # The one-line Impl/Test summary under a module's per-language detail
  # table; a nil test (the gem) points at its external suite instead.
  def module_summary(impl:, test:)
    return "  Impl: #{impl} LOC — the gem's Ruby tests live in test/" if test.nil?

    "  Impl: #{impl}    Inline test: #{test}    Test%: #{test_share(impl, test)}%"
  end

  # A header row, body rows, and a Total footer, all aligned to one
  # shared column width; the tier table and the module roll-up share it.
  def framed_table(header, body, foot)
    widths = [header, foot, *body].transpose.map { |column| column.map(&:length).max }
    rule = "+#{widths.map { |width| "-" * (width + 2) }.join("+")}+"
    [rule, line(header, widths), rule,
     *body.map { |row| line(row, widths) },
     rule, line(foot, widths), rule].join("\n")
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
