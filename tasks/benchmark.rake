# frozen_string_literal: true

# Rake tasks driving the SPEC.md "Regression benchmarks" suite.
# Benchmarks live in benchmark/ and are excluded from the published
# gem; they are quality-pipeline tooling, not gem runtime behaviour.
#
# Variants:
#
#   bench             — #1..#5 (cold_start, rpc_roundtrip, codec,
#                       mruby_eval, handle_table). Cap: 1 MiB on
#                       codec size sweep.
#   bench:smoke       — alias of bench (no fast/slow split yet; the
#                       1 MiB cap is already CI-friendly).
#   bench:full        — bench plus codec @ 16 MiB (BENCH_FULL=1).
#   bench:concurrent  — #6 characterization (not in release gate).
#   bench:memory      — #7 characterization: per-Sandbox RSS, leak
#                       detection, large-payload retention.
#
# Each script writes its suite into
# benchmark/results/<date>-<short-sha>.json; multiple Runner
# instances within one invocation share the same file.
#
# Release-gate benchmark roster lives in tasks/support/kobako_bench.rb.

require_relative "support/kobako_bench"

namespace :bench do
  desc "Run all five regression benchmarks (SPEC.md #1..#5; <=1 MiB payloads)."
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

  desc "Run concurrent characterization benchmark (SPEC.md #6; not in release gate)."
  task :concurrent do
    sh "bundle exec ruby benchmark/concurrent/threads.rb"
  end

  desc "Run memory characterization benchmark (#7; not in release gate)."
  task :memory do
    sh "bundle exec ruby benchmark/memory.rb"
  end
end

desc "Alias for bench:release — the five SPEC regression benchmarks."
task bench: "bench:release"
