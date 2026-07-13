# frozen_string_literal: true

# +rake rbs:lock+ — gate that the gem-sourced RBS pins in
# +rbs_collection.lock.yaml+ still match Gemfile.lock, so a dependency bump
# that forgets +rbs collection update+ fails the build instead of leaking a
# stale-definition warning through every steep run. Its comparator rides
# the tooling suite (+test/tasks/test_rbs_lock.rb+).

require_relative "support/rbs_lock"
require_relative "support/report"

namespace :rbs do
  desc "Verify gem-sourced RBS pins match Gemfile.lock (run `rbs collection update` on drift)."
  task :lock do
    drift = KobakoRbsLock.drift(
      collection_yaml: File.read("rbs_collection.lock.yaml"),
      gemfile_lock: File.read("Gemfile.lock")
    )

    unless drift.empty?
      rows = drift.map { |name, rbs, lock| "  #{name}: rbs #{rbs}, Gemfile.lock #{lock || "(absent)"}" }
      abort "rbs:lock: collection lock drifted from Gemfile.lock — run `rbs collection update`:\n#{rows.join("\n")}"
    end

    puts KobakoReport.gate(name: "rbs:lock", ok_summary: "gem-sourced RBS pins match Gemfile.lock")
  end
end
