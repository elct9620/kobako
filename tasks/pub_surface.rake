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

# Confirmed-deliberate third-party API the in-repo scan cannot see,
# each with the reason it stays public.
PUB_SURFACE_ACKNOWLEDGED = {
  "wasm/kobako-core" => {
    "take_outcome" => "reached via export_guest! expansion ($crate::abi::take_outcome)",
    "ABI_VERSION" => "reached via export_guest! expansion ($crate::abi::ABI_VERSION)"
  }
}.freeze

namespace :stats do
  desc "Report pub items with no in-repo downstream consumer (signal; not in release gate)."
  task :surface do
    PUB_SURFACE_CRATES.each do |crate, consumers|
      sources = FileList["#{crate}/src/**/*.rs"].to_h { |path| [path, File.read(path)] }
      consumers_text = consumers.flat_map { |dir| FileList["#{dir}/src/**/*.rs"].map { |p| File.read(p) } }.join
      unconsumed = KobakoPubSurface.unconsumed(
        KobakoPubSurface.pub_items(sources), consumers_text,
        acknowledged: PUB_SURFACE_ACKNOWLEDGED.fetch(crate, {})
      )
      next if unconsumed.empty?

      puts "#{crate} — pub items with no in-repo downstream consumer:"
      unconsumed.each { |name, location| puts format("  %<name>-24s %<location>s", name: name, location: location) }
    end
    puts "stats:surface: acknowledged #{PUB_SURFACE_ACKNOWLEDGED.sum { |_c, entries| entries.size }} item(s)"
  end
end
