# frozen_string_literal: true

# Rake tasks driving the SPEC.md "Regression benchmarks" suite.
# Benchmarks live in benchmark/ and are excluded from the published
# gem; they are quality-pipeline tooling, not gem runtime behaviour.
#
# Three depths, each a superset of the one above it: +bench+ runs the
# gated set at CI-friendly payload sizes, +bench:full+ adds the 16 MiB
# codec sweep, and +bench:all+ adds every characterization. Each suite
# also has a task of its own for iterating on one probe.
#
# Which suites are gated and which are characterization is the roster's
# to say (benchmark/support/roster.rb), against SPEC.md's Regression
# benchmarks table; `rake -T bench` is the task catalog. Each script
# writes its suite into benchmark/results/<date>-<short-sha>.json, so
# multiple Runner instances within one invocation share a file.
#
# The gate / bless / confirm verbs live behind the Kobako::Bench facade;
# their tasks are in tasks/bench/gate.rake.

require_relative "../../benchmark/support/facade"

namespace :bench do
  desc "Run every gated regression benchmark (SPEC.md Regression benchmarks; <=1 MiB payloads)."
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

  desc "Run the #preload + #run dispatch benchmark on its own (#9; bench:release runs it too)."
  task(:preload_dispatch) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/preload_dispatch.rb" } }

  desc "Run the dispatch-glue isolation benchmark on its own (#10; bench:release runs it too)."
  task(:dispatch_glue) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/dispatch_glue.rb" } }

  desc "Run the host per-invocation benchmark on its own (#12; bench:release runs it too)."
  task(:host_invocation) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/host_invocation.rb" } }

  desc "Run guest-side setup characterization — compile and binding scaling (#13; not in release gate)."
  task(:guest_setup) { Kobako::Bench::Lock.hold { sh "bundle exec ruby benchmark/guest_setup.rb" } }

  # The whole-round sweep for a manual capture: the 16 MiB gated set plus
  # every characterization, merged into one results file. bench:full stays
  # lean and pure-binary for the release gate; bench:all additionally builds
  # the regexp-unicode variant #11 drives. The characterization roster lives
  # in benchmark/support/roster.rb; when a json characterization lands, add
  # its variant prerequisite here and its task there.
  desc "Run the whole sweep: the gated set (16 MiB) plus every characterization."
  task all: ["wasm:build:regexp_unicode"] do
    Kobako::Bench::Lock.hold do
      ["full", *Kobako::Bench::SWEEP_TASKS].each { |suite| Rake::Task["bench:#{suite}"].invoke }
    end
  end
end

desc "Alias for bench:release — every SPEC-gated regression benchmark."
task bench: "bench:release"
