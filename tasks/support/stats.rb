# frozen_string_literal: true

require "json"
require "open3"
require "tempfile"

# Code-statistics helper backing +tasks/stats.rake+ — a rails-stats-style
# size report over the tracked source tree. cloc owns the per-file line
# classification; this module owns file selection (git-tracked, minus
# generated artifacts) and the report shape, so the tier roster
# (+tasks/support/roster.rb+) can grow without touching the counting logic.
module KobakoStats
  module_function

  # Tracked-for-reproducibility artifacts that are not implementation:
  # dependency lock files, vector images, recorded benchmark results,
  # and the +.keep+ placeholders that mount gitignored artifact dirs.
  EXCLUDED = %r{(?:^|/)(?:Cargo|Gemfile)\.lock\z|\.svg\z|/\.keep\z|\Abenchmark/(?:results/|baseline\.json)}

  HEADER = %w[Name Files Lines LOC Comments].freeze

  ZERO_ROW = { files: 0, blank: 0, comment: 0, code: 0 }.freeze

  def cloc_available?
    system("cloc", "--version", out: File::NULL, err: File::NULL)
  end

  def excluded?(path)
    path.match?(EXCLUDED)
  end

  # Count the git-tracked files under +paths+ with cloc, returning one
  # totals row for the tier or per-module roll-up.
  def measure(paths, root:)
    sum(run_cloc(paths, root: root))
  end

  # The per-language rows under +paths+, heaviest first — a single
  # module's composition for its +rake stats:<slug>+ detail table.
  def measure_languages(paths, root:)
    by_language(run_cloc(paths, root: root))
  end

  # Run cloc over the git-tracked files under +paths+, returning its raw
  # +--json+ text (empty when no tracked file matches, which cloc emits
  # nothing for). +--force-lang+ folds +sig/*.rbs+ signatures into Ruby,
  # which cloc does not recognize on its own.
  def run_cloc(paths, root:)
    files = tracked_files(paths, root: root)
    return "" if files.empty?

    Tempfile.create("kobako-stats") do |list|
      list.puts(files)
      list.flush
      output, = Open3.capture2("cloc", "--list-file=#{list.path}", "--json",
                               "--quiet", "--force-lang=Ruby,rbs", chdir: root)
      output
    end
  end

  # The countable files under +paths+: git-tracked (so gitignored build
  # products and vendored trees never enter), minus the exclusion rule.
  def tracked_files(paths, root:)
    output, = Open3.capture2("git", "-C", root, "ls-files", "-z", "--", *paths)
    output.split("\0").reject { |path| excluded?(path) }
  end

  # Fold cloc's +--json+ report into a totals row; cloc emits nothing for
  # an empty file list, so blank output is a zero row.
  def sum(json_text)
    totals = json_text.strip.empty? ? {} : JSON.parse(json_text).fetch("SUM", {})
    { files: totals.fetch("nFiles", 0), blank: totals.fetch("blank", 0),
      comment: totals.fetch("comment", 0), code: totals.fetch("code", 0) }
  end

  # The per-language rows of a cloc +--json+ report — every language
  # section except the +header+ and +SUM+ totals, heaviest first.
  def by_language(json_text)
    report = json_text.strip.empty? ? {} : JSON.parse(json_text)
    rows = report.except("header", "SUM").map do |name, totals|
      { name: name, files: totals.fetch("nFiles", 0), blank: totals.fetch("blank", 0),
        comment: totals.fetch("comment", 0), code: totals.fetch("code", 0) }
    end
    rows.sort_by { |row| [-row[:code], row[:name]] }
  end

  # Render category rows as an aligned table with a Total row and the
  # rails-stats-style code-to-test summary line.
  def table(rows)
    [grid(rows), ratio_line(rows), ""].join("\n")
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
  def ratio_line(rows)
    code = kind_loc(rows, :code)
    test = kind_loc(rows, :test)
    ratio = code.zero? ? 0.0 : test.fdiv(code).round(1)
    "  Code LOC: #{code}    Test LOC: #{test}    Code to Test Ratio: 1:#{ratio}"
  end

  def kind_loc(rows, kind)
    rows.sum { |row| row[:kind] == kind ? row[:code] : 0 }
  end
end
