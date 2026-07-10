# frozen_string_literal: true

# Churn × size × fan-in scorer backing +tasks/hotspots.rake+. A file
# that is large, frequently edited since the last release, and widely
# required is where polish effort pays most; the report is a signal,
# never a gate. Churn windowed to a release tag reads as "unsettled
# since we last shipped" rather than "recently worked on".
module KobakoHotspots
  module_function

  # Per-file edit counts from +git log --name-only --pretty=format:+
  # output, kept to the source trees under +roots+.
  def churn(log_output, roots: %w[lib crates wasm ext])
    pattern = %r{\A(?:#{Regexp.union(roots)})/.*\.(?:rb|rs)\z}
    log_output.each_line(chomp: true).map(&:strip).grep(pattern).tally
  end

  # Reverse +require_relative+ edges over a +{ path => text }+ map of
  # root-relative Ruby sources: how many files require each path.
  def fan_in(sources)
    counts = Hash.new(0)
    sources.each do |path, text|
      text.scan(/require_relative "([^"]+)"/).flatten.each do |rel|
        counts[File.expand_path("#{rel}.rb", "/#{File.dirname(path)}").delete_prefix("/")] += 1
      end
    end
    counts
  end

  # The +limit+ hottest rows, scored by churn × size:
  # +[path, edits, lines, fan_in]+ sorted hottest first.
  def rows(churn:, sizes:, fan_in:, limit: 15)
    scored = churn.filter_map do |path, edits|
      lines = sizes[path]
      [path, edits, lines, fan_in.fetch(path, 0)] if lines
    end
    scored.sort_by { |_path, edits, lines, _fan| -(edits * lines) }.first(limit)
  end
end
