# frozen_string_literal: true

# +rake stats:surface+ — per-crate pub items with no in-repo downstream
# reference. Characterization signal for the published-crate surface
# (not in the release gate): each listed item is either deliberate
# third-party API to acknowledge below, or an over-wide surface to
# demote. The reader's unit coverage rides the test suite
# (+test/tasks/test_pub_surface.rb+).

require_relative "support/pub_surface"

# Analyzed crate => the in-repo source trees that consume it. The two
# frontends (crates/kobako, ext/kobako) are leaves whose surface is the
# product itself, so they are consumers here, never analyzed.
PUB_SURFACE_CRATES = {
  "crates/kobako-codec" => %w[wasm/kobako-core wasm/kobako-mruby wasm/kobako-wasm
                              crates/kobako crates/kobako-parity crates/kobako-wasmtime],
  "crates/kobako-runtime" => %w[crates/kobako-wasmtime crates/kobako ext/kobako],
  "crates/kobako-wasmtime" => %w[crates/kobako ext/kobako],
  "wasm/kobako-core" => %w[wasm/kobako-mruby wasm/kobako-wasm],
  "wasm/kobako-mruby" => %w[wasm/kobako-wasm],
  "wasm/kobako-io" => %w[wasm/kobako-wasm],
  "wasm/kobako-regexp" => %w[wasm/kobako-wasm],
  "wasm/kobako-json" => %w[wasm/kobako-wasm]
}.freeze

# Crates outside the analysis map for a structural reason, not by
# omission; the roster check below holds every other crate to the map.
PUB_SURFACE_EXEMPT = {
  "wasm/kobako-baker" => "standalone bake tool — its lib feeds its own bin, no downstream tree to grep"
}.freeze

# The kobako-mruby bridge cluster is crate-internal to the flows, but
# on mruby-less host builds the flows that use it are compiled out
# (beni placeholder rule) and pub reachability is what keeps the
# dead-code analysis quiet — demoting it trades a clean surface for
# 20+ dead_code warnings or banned #[allow]s.
PUB_SURFACE_BRIDGE_REASON = "placeholder-rule liveness — pub keeps the mruby-less host build " \
                            "warning-free; crate-internal to the flows, not third-party API"

# Pub items confirmed to stay public for a reason the in-repo grep
# cannot see — macro-expanded third-party API, or pub reachability a
# placeholder-rule crate relies on.
PUB_SURFACE_ACKNOWLEDGED = {
  "wasm/kobako-core" => {
    "take_outcome" => "reached via export_guest! expansion ($crate::abi::take_outcome)",
    "ABI_VERSION" => "reached via export_guest! expansion ($crate::abi::ABI_VERSION)"
  },
  "wasm/kobako-mruby" => %w[
    InstallGroupsError Kobako init resolve_raw install_groups raise_transport_error
    raise_service_error collection_len extract_backtrace top_level_constants
    set_handle_id extract_handle_id extract_hash_kwargs unpack_args_kwargs
    to_codec_value try_codec_value
  ].to_h { |name| [name, PUB_SURFACE_BRIDGE_REASON] }
}.freeze

namespace :stats do
  desc "Report pub items with no in-repo downstream consumer (signal; not in release gate)."
  task :surface do
    unaccounted = KobakoPubSurface.unaccounted_crates(
      roster: FileList["{crates,wasm}/*/Cargo.toml"].map { |path| File.dirname(path) },
      analyzed: PUB_SURFACE_CRATES.keys,
      consumers: PUB_SURFACE_CRATES.values.flatten.uniq,
      exempt: PUB_SURFACE_EXEMPT.keys
    )
    abort "stats:surface: unclassified crate(s): #{unaccounted.join(", ")}" unless unaccounted.empty?

    PUB_SURFACE_CRATES.each do |crate, consumers|
      sources = FileList["#{crate}/src/**/*.rs"].to_h { |path| [path, File.read(path)] }
      consumers_text = consumers.flat_map { |dir| FileList["#{dir}/src/**/*.rs"].map { |p| File.read(p) } }.join
      items = KobakoPubSurface.pub_items(sources)
      acknowledged = PUB_SURFACE_ACKNOWLEDGED.fetch(crate, {})
      stale = KobakoPubSurface.stale_acknowledgements(items, acknowledged)
      abort "stats:surface: stale acknowledgement(s) in #{crate}: #{stale.join(", ")}" unless stale.empty?

      unconsumed = KobakoPubSurface.unconsumed(items, consumers_text, acknowledged: acknowledged)
      next if unconsumed.empty?

      puts "#{crate} — pub items with no in-repo downstream consumer:"
      unconsumed.each { |name, location| puts format("  %<name>-24s %<location>s", name: name, location: location) }
    end
    puts "stats:surface: acknowledged #{PUB_SURFACE_ACKNOWLEDGED.sum { |_c, entries| entries.size }} item(s)"
  end
end
