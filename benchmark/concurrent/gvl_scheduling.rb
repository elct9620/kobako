# frozen_string_literal: true

# Characterization benchmark (not in release gate) — the wall-clock
# effect of the per-Sandbox gvl: mode (B-64) on multi-Thread guest
# execution. :release drops Ruby's GVL for the guest span so distinct
# Sandboxes on distinct Threads run their wasm in parallel; :hold
# serializes them. Two opposed workloads bracket where release helps
# and where it hurts:
#
#   compute  — each Thread runs a pure guest loop with no dispatch.
#              Guest work dominates, so :release scales with Thread
#              count while :hold stays serialized; the speedup
#              (hold_seconds / release_seconds) grows with N.
#   dispatch — each Thread runs many guest->host dispatches. Every
#              dispatch re-acquires the GVL, so the handoff cost makes
#              :release match or trail :hold; the speedup stays near 1.
#
# Wall-clock, not CPU time: the point of :release is parallel
# wall-clock progress, which CPU time (summed across cores) cannot
# see — so this suite keeps its own CLOCK_MONOTONIC helper and bypasses
# the CPU-time Runner. Each Thread does a fixed amount of work (weak
# scaling), so under perfect parallelism release_seconds stays flat as
# N grows while hold_seconds climbs.

$LOAD_PATH.unshift File.expand_path("../../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("../support", __dir__)

require "kobako"
require "guest"
require "runner"

GUEST = Kobako::Bench::Guest.path

THREAD_COUNTS = [1, 2, 4, 8].freeze
REPEAT = 12 # guest invocations each Thread runs per measurement

# Pure guest compute, no dispatch — the regime :release parallelizes.
COMPUTE_SCRIPT = <<~RUBY
  acc = 0
  60_000.times { |i| acc = (acc + i * 7) % 1_000_003 }
  acc
RUBY

# Dispatch-bound guest — the regime GVL re-acquisition taxes :release.
DISPATCH_SCRIPT = <<~RUBY
  sum = 0
  300.times { sum = Counter::Bump.call(sum) }
  sum
RUBY

def time_block
  started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  yield
  Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
end

def parallel_join(count)
  Array.new(count) { |i| Thread.new { yield(i) } }.each(&:join)
end

# Build +count+ warmed Sandboxes in +mode+, each bound by +binder+ if given.
# Each Sandbox runs one throwaway eval so its Runtime is warm before the
# timed span, keeping cold-path setup out of the measurement.
def build_sandboxes(mode, count, &binder)
  Array.new(count) do
    Kobako::Sandbox.new(wasm_path: GUEST, gvl: mode).tap do |s|
      binder&.call(s)
      s.eval("nil")
    end
  end
end

# Wall-clock for +count+ Threads to each run +script+ REPEAT times on
# their own Sandbox in +mode+.
def measure(mode, count, script, &binder)
  sandboxes = build_sandboxes(mode, count, &binder)
  time_block { parallel_join(count) { |i| REPEAT.times { sandboxes[i].eval(script) } } }
end

def record(runner, workload, count, hold, release)
  speedup = hold / release
  runner.results << { label: "#{workload}-hold-#{count}", seconds: hold,
                      threads: count, mode: "hold", workload: workload }
  runner.results << { label: "#{workload}-release-#{count}", seconds: release,
                      threads: count, mode: "release", workload: workload }
  runner.results << { label: "#{workload}-speedup-#{count}", ratio: speedup,
                      hold_ms: hold * 1000, release_ms: release * 1000,
                      threads: count, workload: workload }
  puts format("%<w>-8s n=%<n>-2d  hold %<h>8.1f ms  release %<r>8.1f ms  (%<s>.2fx)",
              w: workload, n: count, h: hold * 1000, r: release * 1000, s: speedup)
end

def run_workload(runner, workload, script, &binder)
  THREAD_COUNTS.each do |count|
    hold = measure(:hold, count, script, &binder)
    release = measure(:release, count, script, &binder)
    record(runner, workload, count, hold, release)
  end
end

runner = Kobako::Bench::Runner.new("gvl_scheduling")
Kobako::Sandbox.new(wasm_path: GUEST).eval("nil") # warm process-wide caches

run_workload(runner, "compute", COMPUTE_SCRIPT)
run_workload(runner, "dispatch", DISPATCH_SCRIPT) { |s| s.bind("Counter::Bump", ->(n) { n + 1 }) }

puts runner.write!
