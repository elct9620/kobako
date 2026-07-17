# frozen_string_literal: true

# Pub-surface ledger consistency gate: every acknowledged pub item
# (KobakoPubSurface::ACKNOWLEDGED) must still name a current pub item, so a
# renamed, removed, or demoted item cannot leave dead weight behind. This is
# the deterministic half of the pub-surface scan; the heuristic unconsumed
# report stays a human signal in stats:surface. Reader unit coverage rides
# test/tasks/test_pub_surface.rb.

require_relative "../support/pub_surface"
require_relative "../support/report"

namespace :gate do
  desc "Check the pub-surface acknowledgement ledger names only current pub items."
  task :surface do
    stale = KobakoPubSurface::ACKNOWLEDGED.flat_map do |crate, acknowledged|
      sources = FileList["#{crate}/src/**/*.rs"].to_h { |path| [path, File.read(path)] }
      items = KobakoPubSurface.pub_items(sources)
      KobakoPubSurface.stale_acknowledgements(items, acknowledged).map { |name| "#{crate}: #{name}" }
    end

    acknowledged_count = KobakoPubSurface::ACKNOWLEDGED.sum { |_crate, entries| entries.size }
    puts KobakoReport.gate(name: "gate:surface",
                           ok_summary: "#{acknowledged_count} acknowledgement(s) name a current pub item",
                           violations: stale, noun: "stale acknowledgement")
  end
end
