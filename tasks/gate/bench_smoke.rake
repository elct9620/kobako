# frozen_string_literal: true

# Benchmark probe wiring gate: every probe under benchmark/ still loads
# and every case body still runs. The probes drive the gem's public API
# but nothing else executes them, so a renamed or removed type rots them
# silently — dispatch_glue.rb drove a deleted Transport::Request for
# twenty commits. Each probe runs under the Runner's smoke seam, which
# executes a case body once instead of measuring it, so the check costs
# seconds and answers wiring rather than cost.

require_relative "../support/report"
require_relative "../../benchmark/support/roster"
require_relative "../../benchmark/support/smoke"

# The first build artifact a probe needs and cannot load without, or nil
# when both are present.
def bench_smoke_absent_prereq
  return "the native ext (run `rake compile`)" if Dir[Kobako::Bench::Paths::NATIVE_EXT_GLOB].empty?
  return "data/kobako.wasm (run `rake wasm:build`)" unless File.exist?(Kobako::Bench::Paths::DATA_WASM)

  nil
end

# Probes that stay out of the gate, and why, live in
# benchmark/support/roster.rb; the summary names them so a reader never
# reads this gate as covering everything.
def bench_smoke_summary
  skipped = Kobako::Bench::SMOKE_EXCLUSIONS.keys
  "#{Kobako::Bench::SMOKE_BENCHES.size} probes ran; " \
    "#{skipped.size} not smoked (#{skipped.join(", ")})"
end

namespace :gate do
  namespace :bench do
    desc "Check every benchmark probe still loads and runs (wiring only, no measurement)."
    task :smoke do
      # A clean checkout is missing the artifacts; under CI the default
      # task built them first, so a miss there is a broken pipeline
      # rather than a skip — the call test/support/guest_guard.rb makes
      # for the test suite.
      if (absent = bench_smoke_absent_prereq)
        abort "gate:bench:smoke: #{absent} missing under CI" if ENV["CI"]

        next puts "gate:bench:smoke: SKIP — #{absent} missing."
      end

      env = { Kobako::Bench::Smoke::ENV_NAME => "1" }
      broken = Kobako::Bench::SMOKE_BENCHES.reject do |probe|
        system(env, "bundle exec ruby #{probe}", out: File::NULL)
      end
      puts KobakoReport.gate(name: "gate:bench:smoke", ok_summary: bench_smoke_summary, noun: "broken probe",
                             violations: broken.map { |probe| "#{File.basename(probe)} did not complete" })
    end
  end
end
