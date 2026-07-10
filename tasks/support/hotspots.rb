# frozen_string_literal: true

require_relative "rust_source"

# Churn × size scorer backing +tasks/hotspots.rake+, with fan-in as a
# reference column: a large, frequently edited file is where polish
# effort pays most, and wide require reach raises the stakes without
# entering the score; the report is a signal, never a gate. Churn
# windowed to a release tag reads as "unsettled since we last shipped"
# rather than "recently worked on".
module KobakoHotspots
  module_function

  # Per-file edit counts from +git log --name-only --pretty=format:+
  # output, kept to the source trees under +roots+ — the tier roster is
  # the caller's knowledge, shared with the stats category table so the
  # two instruments cannot drift apart.
  def churn(log_output, roots:)
    pattern = %r{\A(?:#{Regexp.union(roots)})/.*\.(?:rb|rs|rake)\z}
    log_output.each_line(chomp: true).map(&:strip).grep(pattern).tally
  end

  # Reverse +require_relative+ edges over a +{ path => text }+ map of
  # root-relative Ruby sources: how many files require each path. Every
  # scanned source is seeded at zero so absence means "outside the
  # scan", never "no dependents".
  def fan_in(sources)
    counts = sources.keys.to_h { |path| [path, 0] }
    sources.each do |path, text|
      text.scan(/require_relative "([^"]+)"/).flatten.each do |rel|
        target = File.expand_path("#{rel}.rb", "/#{File.dirname(path)}").delete_prefix("/")
        counts[target] = counts.fetch(target, 0) + 1
      end
    end
    counts
  end

  # The size a hotspot is judged by: implementation lines only. Rust
  # carries its tests inline while Ruby's live in the excluded test/
  # tree, so a .rs file sheds its +#[cfg(test)]+ tail before entering
  # the cross-language churn × size ranking.
  def impl_lines(path, text)
    body = path.end_with?(".rs") ? KobakoRustSource.impl_body(text) : text
    body.lines.count
  end

  # The +limit+ hottest rows, scored by churn × size:
  # +[path, edits, lines, fan_in]+ sorted hottest first, +nil+ fan-in
  # marking a path the fan-in scan does not measure.
  def rows(churn:, sizes:, fan_in:, limit: 15)
    scored = churn.filter_map do |path, edits|
      lines = sizes[path]
      [path, edits, lines, fan_in[path]] if lines
    end
    scored.sort_by { |_path, edits, lines, _fan| -(edits * lines) }.first(limit)
  end
end
