# frozen_string_literal: true

# +rake stats:surface+ — per-crate pub items with no in-repo downstream
# reference. Characterization signal for the published-crate surface
# (not in the release gate): each listed item is either deliberate
# third-party API to add to the acknowledgement ledger
# (+KobakoPubSurface::ACKNOWLEDGED+, whose consistency +gate:surface+
# enforces), or an over-wide surface to demote. The reader's unit coverage
# rides the test suite (+test/tasks/test_pub_surface.rb+).

require_relative "support/pub_surface"
require_relative "support/roster"
require_relative "support/report"

# Analyzed crate => consumers is derived from the Cargo.toml path
# dependencies (transitively, so consumption through a re-exporting
# frontend still counts); a leaf with no in-repo dependent — the
# frontends, the parity runner, the baker — is never analyzed. The
# crate trees come from the shared tier roster, so a new code tier
# enters this scan the day it enters the roster.
PUB_SURFACE_MANIFESTS = FileList[KobakoRoster.tier_paths(%i[code]).map { |root| "#{root}/*/Cargo.toml" }]

namespace :stats do
  desc "Report pub items with no in-repo downstream consumer (signal; not in release gate)."
  task :surface do
    manifests = PUB_SURFACE_MANIFESTS.to_h { |path| [File.dirname(path), File.read(path)] }
    graph = KobakoPubSurface.transitive_consumers(KobakoPubSurface.path_dependencies(manifests))

    puts KobakoReport.banner("stats:surface — pub items with no in-repo consumer",
                             reads_as: "a signal, not a gate; an acknowledged item is intended third-party API")

    # Ledger staleness is the gate's job (gate:surface); the signal stays a
    # report that always runs to completion, acknowledged names simply skipped.
    graph.each do |crate, consumers|
      sources = FileList["#{crate}/src/**/*.rs"].to_h { |path| [path, File.read(path)] }
      consumers_text = consumers.flat_map { |dir| FileList["#{dir}/src/**/*.rs"].map { |p| File.read(p) } }.join
      items = KobakoPubSurface.pub_items(sources)
      acknowledged = KobakoPubSurface::ACKNOWLEDGED.fetch(crate, {})

      unconsumed = KobakoPubSurface.unconsumed(items, consumers_text, acknowledged: acknowledged)
      next if unconsumed.empty?

      puts "#{crate} — pub items with no in-repo downstream consumer:"
      unconsumed.each { |name, location| puts format("  %<name>-24s %<location>s", name: name, location: location) }
    end
    puts "stats:surface: acknowledged #{KobakoPubSurface::ACKNOWLEDGED.sum { |_c, entries| entries.size }} item(s)"
  end
end
