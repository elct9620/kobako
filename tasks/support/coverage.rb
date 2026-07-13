# frozen_string_literal: true

# Pure-Ruby reporter backing +tasks/coverage.rake+. Walks the stdlib
# +Coverage+ result into per-file line-coverage report lines scoped to
# +lib/kobako/+, worst-covered first; the rake task owns the printing.
# The scope is Ruby lines only — the Rust host and guest are measured
# separately, and the banner says so — so this report is never mistaken
# for whole-system coverage. Characterization tooling — not part of the
# release gate, no thresholds enforced.
module KobakoCoverage
  module_function

  # Absolute path of the source tree the reporter measures. Anything
  # outside this prefix (e.g. stdlib, third-party gems, +sig/+) is
  # filtered out of the report.
  LIB_ROOT = File.expand_path("../../lib/kobako", __dir__)

  # The printable report for +result+ — the hash returned by
  # +Coverage.result+ keyed by absolute source path.
  def report_lines(result)
    entries = collect(result)
    return ["No lib/kobako/ source files were loaded — empty suite?"] if entries.empty?

    width = column_width(entries)
    [*header_lines(entries.size, width),
     *entries.map { |entry| row_line(entry, width) },
     *total_lines(entries, width)]
  end

  # Convert a +Coverage.result+ hash into a sorted array of per-file
  # entries (lowest coverage first; ties broken by name). Entries
  # outside +LIB_ROOT+ or with no executable lines are excluded.
  def collect(result)
    result.filter_map { |path, hits| entry_for(path, hits) }.sort_by { |e| [e[:pct], e[:name]] }
  end

  # Build the per-file entry hash, or +nil+ when +path+ is outside
  # +LIB_ROOT+ or has no executable lines. Each hash carries
  # +name+ / +covered+ / +relevant+ / +pct+ / +uncov+.
  def entry_for(path, hits)
    return nil unless path.start_with?(LIB_ROOT)

    relevant = hits.compact
    relevant.empty? ? nil : build_entry(path, hits, relevant)
  end

  def build_entry(path, hits, relevant)
    covered = relevant.count(&:positive?)
    {
      name: path.sub("#{LIB_ROOT}/", ""),
      covered: covered,
      relevant: relevant.size,
      pct: (covered.to_f / relevant.size * 100).round(1),
      uncov: uncovered_lines(hits)
    }
  end

  # The 1-based line numbers where +Coverage+ reported zero hits.
  # +nil+ entries (blank lines, comments, non-executable code) are
  # skipped.
  def uncovered_lines(hits)
    hits.each_with_index.filter_map { |h, i| h.is_a?(Integer) && h.zero? ? i + 1 : nil }
  end

  # The report banner names its scope up front: these percentages are
  # Ruby line coverage of +lib/kobako/+ alone. The Rust host and guest
  # carry no line instrument here, and a Ruby line that calls into the
  # native ext counts as covered the moment it runs — regardless of the
  # Rust path behind it — so the total must never be read as
  # whole-system coverage.
  def header_lines(count, width)
    rule = "=" * (width + 32)
    ["", rule,
     "Coverage — Ruby lib/kobako/ lines only  (#{count} files)",
     "Rust host/guest not measured here; an ext-boundary line reads as covered once it runs.",
     rule]
  end

  # A single table row. Uncovered line numbers ride at the end,
  # truncated to the first eight with an ellipsis when the list is
  # longer — keeps the row scannable while still pointing the reader
  # at where to look.
  def row_line(entry, width)
    base = "#{entry[:name].ljust(width)}  " \
           "#{entry[:covered].to_s.rjust(3)}/#{entry[:relevant].to_s.rjust(3)}  " \
           "#{entry[:pct].to_s.rjust(5)}%"
    return base if entry[:uncov].empty?

    "#{base}  uncovered: #{format_uncov(entry[:uncov])}"
  end

  def format_uncov(lines)
    sample = lines.first(8).join(",")
    lines.size > 8 ? "#{sample}…" : sample
  end

  def total_lines(entries, width)
    totals = compute_totals(entries)
    ["-" * (width + 32),
     "TOTAL: #{totals[:covered]}/#{totals[:relevant]} (#{totals[:pct]}%)",
     "=" * (width + 32)]
  end

  def compute_totals(entries)
    relevant = entries.sum { |e| e[:relevant] }
    covered = entries.sum { |e| e[:covered] }
    { covered: covered, relevant: relevant, pct: (covered.to_f / relevant * 100).round(1) }
  end

  def column_width(entries)
    entries.map { |e| e[:name].length }.max
  end
end
