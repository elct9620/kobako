# frozen_string_literal: true

# Release-gate arbitration for the regression benchmarks: the anchored
# comparison against benchmark/baseline.json, the deliberate re-bless of
# that anchor, and the stage-2 paired-alternation arbiter. The benchmark
# runs themselves live in tasks/bench/run.rake.

require_relative "../support/bench/confirm"
require_relative "../support/bench/gate"

namespace :bench do
  desc "Anchored release gate: compare a run against benchmark/baseline.json (or args [current,baseline])."
  task(:gate, %i[current baseline]) { |_t, args| KobakoBench::Gate.gate!(args[:current], args[:baseline]) }

  desc "Re-bless the anchor (benchmark/baseline.json) from a run; document the reason in the benchmark README."
  task(:bless, %i[run]) { |_t, args| KobakoBench::Gate.bless!(args[:run]) }

  desc "Stage-2 arbiter: paired alternation against a released Guest Binary (version or wasm path)."
  task(:confirm, %i[baseline]) { |_t, args| KobakoBench::Confirm.confirm!(args[:baseline]) }

  desc "Run the release-gate unit tests (comparator + runner)."
  task :gate_test do
    Dir["tasks/support/bench/*_test.rb"].each { |file| sh "bundle exec ruby #{file}" }
  end
end
