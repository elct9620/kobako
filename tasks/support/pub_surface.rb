# frozen_string_literal: true

require_relative "rust_source"

# Pub-surface consumption reader and acknowledgement ledger, backing the
# +stats:surface+ signal and the +gate:surface+ consistency gate. A +pub+
# item with no in-repo downstream reference is either deliberate
# third-party API (acknowledged below with a reason) or an over-wide
# surface to demote — the signal lists the unreviewed remainder for a human
# call, and the gate fails on any acknowledgement a current pub item no
# longer carries.
module KobakoPubSurface
  module_function

  # The kobako-mruby bridge cluster is crate-internal to the flows, but on
  # mruby-less host builds the flows that use it are compiled out (beni
  # placeholder rule) and pub reachability is what keeps the dead-code
  # analysis quiet — demoting it trades a clean surface for 20+ dead_code
  # warnings or banned #[allow]s.
  BRIDGE_REASON = "placeholder-rule liveness — pub keeps the mruby-less host build " \
                  "warning-free; crate-internal to the flows, not third-party API"

  # Pub items confirmed to stay public for a reason the in-repo grep cannot
  # see — macro-expanded third-party API, or pub reachability a
  # placeholder-rule crate relies on. gate:surface fails the day an entry
  # names an item no current pub surface carries.
  ACKNOWLEDGED = {
    "crates/kobako" => {
      "YieldError" => "SDK third-party API — the yield-arm error embedders match on; " \
                      "the in-repo parity runner never names it"
    },
    "wasm/kobako-core" => {
      "take_outcome" => "reached via export_guest! expansion ($crate::abi::take_outcome)",
      "ABI_VERSION" => "reached via export_guest! expansion ($crate::abi::ABI_VERSION)"
    },
    "wasm/kobako-mruby" => %w[
      InstallError install_bindings Kobako init resolve_raw raise_transport_error
      raise_service_error extract_backtrace top_level_constants set_handle_id
      extract_handle_id
    ].to_h { |name| [name, BRIDGE_REASON] }
  }.freeze

  # A crate's public Rust surface; +pub(crate)+ and narrower stay out,
  # as do the +pub extern "C"+ templates inside +export_guest!+ — those
  # expand in the consumer's crate, not here. +pub use+ / +pub mod+ are
  # namespace shapes, not surface: their consumption is their items',
  # counted where each item is defined.
  PUB_ITEM = /^\s*pub (?:(?:unsafe|const|async) )*(?:fn|struct|enum|trait|const|type|static(?: mut)?) (\w+)/

  # An exported macro — the attribute is what makes a macro public
  # surface; a bare +macro_rules!+ stays crate-internal.
  MACRO_ITEM = /^\s*#\[macro_export\]\s*\n\s*macro_rules!\s+(\w+)/

  # +[[name, "path:line"], ...]+ for every pub item and exported macro
  # in a +{ path => text }+ map, with each file's +#[cfg(test)]+ tail
  # module excluded via the shared source-shape rule.
  def pub_items(sources)
    sources.flat_map do |path, text|
      body = KobakoRustSource.impl_body(text)
      (scan_items(body) + scan_macros(body)).sort.map { |lineno, name| [name, "#{path}:#{lineno}"] }
    end
  end

  def scan_items(body)
    body.each_line.with_index(1).filter_map do |line, lineno|
      name = line[PUB_ITEM, 1]
      [lineno, name] if name
    end
  end

  # Each macro surfaces at its +macro_rules!+ line, one below the
  # attribute the match anchors on.
  def scan_macros(body)
    body.enum_for(:scan, MACRO_ITEM).map do
      match = Regexp.last_match
      [match.pre_match.count("\n") + 2, match[1]]
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
  # what the acknowledgement ledger records — and a same-named
  # reference to a different item reads as consumption, so a listed
  # item is a lead to verify, never a verdict.
  def unconsumed(items, consumers_text, acknowledged: {})
    items.reject do |name, _location|
      acknowledged.key?(name) || consumers_text.match?(/\b#{Regexp.escape(name)}\b/)
    end
  end
end
