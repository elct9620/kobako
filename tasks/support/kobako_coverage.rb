# frozen_string_literal: true

# Pure-Ruby reporter backing +tasks/coverage.rake+. Walks the stdlib
# +Coverage+ result and prints a per-file line-coverage table scoped
# to +lib/kobako/+. Characterization tooling — not part of the
# release gate, no thresholds enforced.
module KobakoCoverage
  module_function

  # Absolute path of the source tree the reporter measures. Anything
  # outside this prefix (e.g. stdlib, third-party gems, +sig/+) is
  # filtered out of the report.
  LIB_ROOT = File.expand_path("../../lib/kobako", __dir__)

  # Render the coverage report for +result+ — the hash returned by
  # +Coverage.result+ keyed by absolute source path.
  def report(result)
    entries = collect(result)
    return puts "No lib/kobako/ source files were loaded — empty suite?" if entries.empty?

    print_table(entries)
    print_totals(entries)
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

  # Materialise the per-file entry once +entry_for+ has confirmed the
  # path is in scope and has executable lines. Split out to keep
  # +entry_for+ inside Rubocop's MethodLength budget.
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

  # Return the 1-based line numbers where +Coverage+ reported zero
  # hits. +nil+ entries (blank lines, comments, non-executable code)
  # are skipped.
  def uncovered_lines(hits)
    hits.each_with_index.filter_map { |h, i| h.is_a?(Integer) && h.zero? ? i + 1 : nil }
  end

  def print_table(entries)
    width = column_width(entries)
    puts ""
    puts "=" * (width + 32)
    puts "Coverage — lib/kobako/  (#{entries.size} files)"
    puts "=" * (width + 32)
    entries.each { |e| puts format_row(e, width) }
  end

  # Format a single table row. Uncovered line numbers ride at the end
  # of the row, truncated to the first eight with an ellipsis when
  # the list is longer — keeps the row scannable while still pointing
  # the reader at where to look.
  def format_row(entry, width)
    base = format_base(entry, width)
    return base if entry[:uncov].empty?

    "#{base}  uncovered: #{format_uncov(entry[:uncov])}"
  end

  def format_base(entry, width)
    "#{entry[:name].ljust(width)}  " \
      "#{entry[:covered].to_s.rjust(3)}/#{entry[:relevant].to_s.rjust(3)}  " \
      "#{entry[:pct].to_s.rjust(5)}%"
  end

  def format_uncov(lines)
    sample = lines.first(8).join(",")
    lines.size > 8 ? "#{sample}…" : sample
  end

  def print_totals(entries)
    totals = compute_totals(entries)
    width = column_width(entries)
    puts "-" * (width + 32)
    puts "TOTAL: #{totals[:covered]}/#{totals[:relevant]} (#{totals[:pct]}%)"
    puts "=" * (width + 32)
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
