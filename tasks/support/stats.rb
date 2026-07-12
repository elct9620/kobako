# frozen_string_literal: true

require "json"
require "open3"
require "tempfile"

require_relative "rust_source"
require_relative "stats/report"

# The measurement half of +KobakoStats+ backing +tasks/stats.rake+
# (+tasks/support/stats/report.rb+ renders the rows it yields): cloc owns
# the per-file line classification; this file owns file selection
# (git-tracked, minus generated artifacts) and the cloc aggregation, so
# the tier roster (+tasks/support/roster.rb+) can grow without touching
# the counting logic.
module KobakoStats
  module_function

  # Tracked-for-reproducibility artifacts that are not implementation:
  # dependency lock files, vector images, recorded benchmark results,
  # and the +.keep+ placeholders that mount gitignored artifact dirs.
  EXCLUDED = %r{(?:^|/)(?:Cargo|Gemfile)\.lock\z|\.svg\z|/\.keep\z|\Abenchmark/(?:results/|baseline\.json)}

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

  # The code-LOC of the Rust inline +#[cfg(test)]+ tails under +paths+,
  # which cloc otherwise folds into each crate's production count because
  # a test module shares its source file. Splitting every +.rs+ at its
  # test module (+KobakoRustSource+) and clocing the tails alone recovers
  # the figure the code-to-test ratio needs to count inline tests as
  # tests.
  def rust_test_loc(paths, root:)
    tails = tracked_files(paths, root: root).filter_map do |rel|
      next unless rel.end_with?(".rs")

      body = File.read(File.join(root, rel))
      tail = body.delete_prefix(KobakoRustSource.impl_body(body))
      tail unless tail.empty?
    end
    return 0 if tails.empty?

    sum(cloc_text(tails.join("\n"), suffix: ".rs"))[:code]
  end

  # cloc's report for a blob of source assembled in memory, written to a
  # temp file whose +suffix+ cloc reads the language from — the way to
  # count the concatenated Rust test tails, which have no path of their
  # own.
  def cloc_text(text, suffix:)
    Tempfile.create(["kobako-stats", suffix]) do |file|
      file.write(text)
      file.flush
      output, = Open3.capture2("cloc", file.path, "--json", "--quiet")
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
end
