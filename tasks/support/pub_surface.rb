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
  # expand in the consumer's crate, not here. +pub use+ / +pub mod+ /
  # +pub static+ are deliberate exclusions: re-exports are caught at
  # their definition site, and no real source holds the other two.
  PUB_ITEM = /^\s*pub (?:(?:unsafe|const|async) )*(?:fn|struct|enum|trait|const|type) (\w+)/

  # A +#[cfg(test)]+ gate that opens a test module — the only shape that
  # truncates the scan; an inline cfg(test) item must not hide the
  # public surface that follows it.
  TEST_MODULE = /^\s*#\[cfg\(test\)\]\s*\n\s*mod\b/

  # +[[name, "path:line"], ...]+ for every pub item in a
  # +{ path => text }+ map, with each file's +#[cfg(test)]+ tail module
  # excluded (test modules sit at the end of a file by convention).
  def pub_items(sources)
    sources.flat_map do |path, text|
      body = text.split(TEST_MODULE, 2).first
      body.each_line.with_index(1).filter_map do |line, lineno|
        name = line[PUB_ITEM, 1]
        [name, "#{path}:#{lineno}"] if name
      end
    end
  end

  # In-repo dependency edges from a +{ crate_dir => manifest_text }+
  # map of Cargo.toml sources — the graph cargo actually links, so the
  # consumer map cannot drift behind the repo. Only an inline-table
  # dependency carries an edge; a bare +path+ line names a build target,
  # never a dependency.
  def path_dependencies(manifests)
    manifests.to_h do |dir, text|
      deps = text.scan(/^\s*[\w-]+\s*=\s*\{[^}]*path\s*=\s*"([^"]+)"/).flatten
      [dir, deps.map { |rel| File.expand_path(rel, "/#{dir}").delete_prefix("/") }.uniq]
    end
  end

  # +{ crate_dir => [dependent dirs] }+ for every crate at least one
  # edge consumes, closed transitively so consumption through a
  # re-exporting middle crate still counts; a crate with no dependent
  # is a leaf whose surface is the product itself, never analyzed.
  def transitive_consumers(edges)
    direct = Hash.new { |hash, dep| hash[dep] = [] }
    edges.each { |consumer, deps| deps.each { |dep| direct[dep] << consumer } }
    direct.keys.sort.to_h { |dep| [dep, expand_consumers(dep, direct).sort] }
  end

  def expand_consumers(dep, direct, seen = Set.new)
    direct[dep].each do |consumer|
      next if seen.include?(consumer)

      seen << consumer
      expand_consumers(consumer, direct, seen)
    end
    seen.to_a
  end

  # The acknowledged names no current pub item carries — the ledger's
  # staleness half, mirroring the Pending-anchors rule so a renamed or
  # demoted item cannot leave dead weight behind.
  def stale_acknowledgements(items, acknowledged)
    acknowledged.keys - items.map { |name, _location| name }
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
