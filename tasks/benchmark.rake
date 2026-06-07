# frozen_string_literal: true

# Rake tasks driving the SPEC.md "Regression benchmarks" suite.
# Benchmarks live in benchmark/ and are excluded from the published
# gem; they are quality-pipeline tooling, not gem runtime behaviour.
#
# Variants:
#
#   bench             — #1..#6 (cold_start, transport_roundtrip, codec,
#                       mruby_eval, catalog_handles, yield_roundtrip).
#                       Cap: 1 MiB on codec size sweep.
#   bench:smoke       — alias of bench (no fast/slow split yet; the
#                       1 MiB cap is already CI-friendly).
#   bench:full        — bench plus codec @ 16 MiB (BENCH_FULL=1).
#   bench:concurrent        — #7 characterization (not in release gate).
#   bench:memory            — #8 characterization: per-Sandbox RSS,
#                             leak detection, large-payload retention.
#   bench:preload_dispatch  — #9 characterization: #preload + #run
#                             setup-once / dispatch-many path
#                             (not in release gate).
#
# Each script writes its suite into
# benchmark/results/<date>-<short-sha>.json; multiple Runner
# instances within one invocation share the same file.
#
# Release-gate benchmark roster lives in tasks/support/kobako_bench.rb.

require_relative "support/kobako_bench"
require_relative "support/kobako_bench_confirm"
require_relative "support/kobako_bench_gate"

namespace :bench do
  desc "Run all six regression benchmarks (SPEC.md #1..#6; <=1 MiB payloads)."
  task :release do
    KobakoBench::RELEASE_BENCHES.each { |script| sh "bundle exec ruby #{script}" }
  end

  desc "Same as bench:release — CI-friendly, no extra-large payloads."
  task smoke: :release

  desc "Run regression benchmarks including 16 MiB codec payload."
  task :full do
    ENV["BENCH_FULL"] = "1"
    Rake::Task["bench:release"].invoke
  end

  desc "Run concurrent characterization benchmark (#7; not in release gate)."
  task :concurrent do
    sh "bundle exec ruby benchmark/concurrent/threads.rb"
  end

  desc "Run memory characterization benchmark (#8; not in release gate)."
  task :memory do
    sh "bundle exec ruby benchmark/memory.rb"
  end

  desc "Run #preload + #run dispatch characterization (#9; not in release gate)."
  task :preload_dispatch do
    sh "bundle exec ruby benchmark/preload_dispatch.rb"
  end
end

namespace :bench do
  desc "Anchored release gate: compare a run against benchmark/baseline.json (or args [current,baseline])."
  task(:gate, %i[current baseline]) { |_t, args| KobakoBench::Gate.gate!(args[:current], args[:baseline]) }

  desc "Re-bless the anchor (benchmark/baseline.json) from a run; document the reason in the benchmark README."
  task(:bless, %i[run]) { |_t, args| KobakoBench::Gate.bless!(args[:run]) }

  desc "Stage-2 arbiter: paired alternation against a released Guest Binary (version or wasm path)."
  task(:confirm, %i[baseline]) { |_t, args| KobakoBench::Confirm.confirm!(args[:baseline]) }

  desc "Run the release-gate unit tests (comparator + runner)."
  task :gate_test do
    Dir["tasks/support/kobako_bench_*_test.rb"].each { |file| sh "bundle exec ruby #{file}" }
  end
end

desc "Alias for bench:release — the six SPEC regression benchmarks."
task bench: "bench:release"
