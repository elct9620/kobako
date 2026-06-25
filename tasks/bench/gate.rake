# frozen_string_literal: true

# Release-gate arbitration for the regression benchmarks: the anchored
# comparison against benchmark/baseline.json, the deliberate re-bless of
# that anchor, and the stage-2 paired-alternation arbiter. The judgment
# itself lives behind the Kobako::Bench facade; the benchmark runs live
# in tasks/bench/run.rake.

require_relative "../../benchmark/support/facade"

namespace :bench do
  desc "Anchored release gate: compare a run against benchmark/baseline.json (or args [current,baseline])."
  task(:gate, %i[current baseline]) { |_t, args| Kobako::Bench.gate(args[:current], args[:baseline]) }

  desc "Re-bless the anchor (benchmark/baseline.json) from a run; document the reason in the benchmark README."
  task(:bless, %i[run]) { |_t, args| Kobako::Bench.bless(args[:run]) }

  desc "Stage-2 arbiter: paired alternation against a released Guest Binary (version or wasm path)."
  task(:confirm, %i[baseline]) { |_t, args| Kobako::Bench.confirm(args[:baseline]) }

  desc "Run the release-gate unit tests (comparator + runner)."
  task :gate_test do
    Dir["benchmark/support/*_test.rb"].each { |file| sh "bundle exec ruby #{file}" }
  end

  desc "Render a head-vs-base benchmark comparison as Markdown (PR job summary)."
  task(:report, %i[current baseline]) do |_t, args|
    puts Kobako::Bench.report(args.fetch(:current), args.fetch(:baseline))
  end
end
