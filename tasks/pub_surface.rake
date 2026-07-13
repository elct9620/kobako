# frozen_string_literal: true

# +rake stats:surface+ — per-crate pub items with no in-repo downstream
# reference. Characterization signal for the published-crate surface
# (not in the release gate): each listed item is either deliberate
# third-party API to acknowledge below, or an over-wide surface to
# demote. The reader's unit coverage rides the test suite
# (+test/tasks/test_pub_surface.rb+).

require_relative "support/pub_surface"
require_relative "support/roster"

# Analyzed crate => consumers is derived from the Cargo.toml path
# dependencies (transitively, so consumption through a re-exporting
# frontend still counts); a leaf with no in-repo dependent — the
# frontends, the parity runner, the baker — is never analyzed. The
# crate trees come from the shared tier roster, so a new code tier
# enters this scan the day it enters the roster.
PUB_SURFACE_MANIFESTS = FileList[KobakoRoster.tier_paths(%i[code]).map { |root| "#{root}/*/Cargo.toml" }]

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
    extract_handle_id extract_hash_kwargs unpack_args_kwargs to_codec_value
    try_codec_value
  ].to_h { |name| [name, PUB_SURFACE_BRIDGE_REASON] }
}.freeze

namespace :stats do
  desc "Report pub items with no in-repo downstream consumer (signal; not in release gate)."
  task :surface do
    manifests = PUB_SURFACE_MANIFESTS.to_h { |path| [File.dirname(path), File.read(path)] }
    graph = KobakoPubSurface.transitive_consumers(KobakoPubSurface.path_dependencies(manifests))

    graph.each do |crate, consumers|
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
