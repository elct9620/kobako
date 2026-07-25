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
#   bench:gvl_scheduling    — gvl: hold-vs-release wall-clock scaling
#                             (not in release gate). Confirmation half of
#                             the GVL-impact toolkit alongside #7 / #10.
#   bench:memory            — #8 characterization: per-Sandbox RSS,
#                             leak detection, large-payload retention.
#   bench:preload_dispatch  — #9 characterization: #preload + #run
#                             setup-once / dispatch-many path
#                             (not in release gate).
#   bench:dispatch_glue     — #10 characterization: GVL-held host glue
#                             of one guest->host dispatch, isolated from
#                             wasm. Predictive half of the GVL-impact
#                             toolkit (#7 is the confirmation half).
#   bench:regexp            — #11 characterization: regexp compile / match /
#                             operations on the regexp-unicode variant.
#   bench:all               — the whole-round sweep: bench:full plus every
#                             characterization (#7-#11), one merged file.
#
# Each script writes its suite into
# benchmark/results/<date>-<short-sha>.json; multiple Runner
# instances within one invocation share the same file.
#
# The benchmark roster and the gate / bless / confirm verbs both live
# behind the Kobako::Bench facade; the anchored gate tasks are in
# tasks/bench/gate.rake.

require_relative "../../benchmark/support/facade"

namespace :bench do
  desc "Run all six regression benchmarks (SPEC.md #1..#6; <=1 MiB payloads)."
  task :release do
    Kobako::Bench::Lock.hold do
      Kobako::Bench::RELEASE_BENCHES.each { |script| sh "bundle exec ruby #{script}" }
    end
  end

  desc "Same as bench:release — CI-friendly, no extra-large payloads."
  task smoke: :release

  desc "Run regression benchmarks including 16 MiB codec payload."
  task :full do
    Kobako::Bench::Lock.hold do
      ENV["BENCH_FULL"] = "1"
      Rake::Task["bench:release"].invoke
    end
  end

  desc "Run concurrent characterization benchmark (#7; not in release gate)."
  task(:concurrent) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/concurrent/threads.rb" } }

  desc "Run gvl: hold-vs-release scaling characterization (not in release gate)."
  task(:gvl_scheduling) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/concurrent/gvl_scheduling.rb" } }

  desc "Run memory characterization benchmark (#8; not in release gate)."
  task(:memory) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/memory.rb" } }

  desc "Run #preload + #run dispatch characterization (#9; not in release gate)."
  task(:preload_dispatch) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/preload_dispatch.rb" } }

  desc "Run dispatch-glue isolation characterization (#10; not in release gate)."
  task(:dispatch_glue) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/dispatch_glue.rb" } }

  desc "Run host per-invocation cost characterization (#12; not in release gate)."
  task(:host_invocation) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/host_invocation.rb" } }

  # The whole-round sweep for a manual capture: the 16 MiB gated set plus every
  # characterization (#7-#12), merged into one results file. bench:full stays
  # lean and pure-binary for the release gate; bench:all additionally builds the
  # regexp-unicode variant #11 drives. When a json characterization lands, add
  # its variant prerequisite and suite here.
  desc "Run the whole sweep: gated (16 MiB) + every characterization (#7-#12)."
  task all: ["wasm:build:regexp_unicode"] do
    Kobako::Bench::Lock.hold do
      %w[full concurrent gvl_scheduling memory preload_dispatch dispatch_glue host_invocation
         regexp].each do |suite|
        Rake::Task["bench:#{suite}"].invoke
      end
    end
  end
end

desc "Alias for bench:release — the six SPEC regression benchmarks."
task bench: "bench:release"
