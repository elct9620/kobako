# frozen_string_literal: true

# Pub-surface consumption reader backing +tasks/pub_surface.rake+. A
# +pub+ item with no in-repo downstream reference is either deliberate
# third-party API (acknowledged with a reason) or an over-wide surface
# to demote — the report lists the unreviewed remainder for a human
# call, never gates.
module KobakoPubSurface
  module_function

  # A crate's public Rust surface; +pub(crate)+ and narrower stay out,
  # as do the +pub extern "C"+ templates inside +export_guest!+ — those
  # expand in the consumer's crate, not here.
  PUB_ITEM = /^\s*pub (?:(?:unsafe|const|async) )*(?:fn|struct|enum|trait|const|type) (\w+)/

  # +[[name, "path:line"], ...]+ for every pub item in a
  # +{ path => text }+ map, with each file's +#[cfg(test)]+ tail
  # excluded (test modules sit at the end of a file by convention).
  def pub_items(sources)
    sources.flat_map do |path, text|
      test_at = text.index("#[cfg(test)]")
      body = test_at ? text[0...test_at] : text
      body.each_line.with_index(1).filter_map do |line, lineno|
        name = line[PUB_ITEM, 1]
        [name, "#{path}:#{lineno}"] if name
      end
    end
  end

  # The items with no word-boundary reference anywhere in
  # +consumers_text+ and no acknowledgement entry. Macro-expanded
  # consumption (+$crate::+ paths) is invisible to this scan — that is
  # what the acknowledgement ledger records.
  def unconsumed(items, consumers_text, acknowledged: {})
    items.reject do |name, _location|
      acknowledged.key?(name) || consumers_text.match?(/\b#{Regexp.escape(name)}\b/)
    end
  end
end
