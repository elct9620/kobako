# frozen_string_literal: true

# Characterization benchmark (not in release gate) — measures
# behaviour under multi-Thread Sandbox usage, the typical Sidekiq /
# Puma cluster shape. SPEC.md does not currently specify a
# concurrency contract; this benchmark observes the status quo so
# future ext/ changes (e.g. introducing rb_thread_call_without_gvl)
# can be compared before/after.
#
#   6a — N Threads each owning a Sandbox, running #run in parallel.
#        Under Ruby's GVL with no GVL-release call in ext/, total
#        throughput is expected to remain flat across N.
#   6b — N Threads each calling Sandbox.new cold. Measures mutex
#        contention on the shared MODULE_CACHE in
#        ext/kobako/src/wasm/cache.rs.
#   6c — Head-of-line blocking: one Thread running a long #run; a
#        second Thread tries to start its own #run("nil"). Latency
#        of the second Thread is the GVL-hold span of the first.

$LOAD_PATH.unshift File.expand_path("../../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("../support", __dir__)

require "kobako"
require "runner"

OPS_PER_THREAD_6A = 50
LONG_SCRIPT = <<~RUBY
  acc = 0
  500_000.times { |i| acc ^= i }
  acc
RUBY

def time_block
  started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  yield
  Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
end

def parallel_join(count)
  Array.new(count) { |i| Thread.new { yield(i) } }.each(&:join)
end

def measure_6a(runner, count)
  sandboxes = Array.new(count) { Kobako::Sandbox.new }
  sandboxes.each { |s| s.run("nil") }
  elapsed = time_block { parallel_join(count) { |i| OPS_PER_THREAD_6A.times { sandboxes[i].run("nil") } } }
  total = count * OPS_PER_THREAD_6A
  runner.results << { label: "6a-threads-#{count}", seconds: elapsed,
                      ops: total, ops_per_sec: total / elapsed, mode: "concurrent" }
  puts format("6a-threads-%<n>-3d %<rate>12.1f ops/s", n: count, rate: total / elapsed)
end

def measure_6b(runner, count)
  elapsed = time_block { parallel_join(count) { Kobako::Sandbox.new } }
  runner.results << { label: "6b-new-#{count}", seconds: elapsed,
                      constructions: count, per_construction_seconds: elapsed / count,
                      mode: "concurrent" }
  puts format("6b-new-%<n>-3d %<sec>12.3f ms (%<per>.3f ms each)",
              n: count, sec: elapsed * 1000, per: (elapsed / count) * 1000)
end

def measure_6c(runner)
  short = Kobako::Sandbox.new.tap { |s| s.run("nil") }
  long = Kobako::Sandbox.new.tap { |s| s.run("nil") }
  baseline = time_block { short.run("nil") }
  contended = run_under_contention(long, short)
  record_6c(runner, baseline, contended)
end

def run_under_contention(long_sandbox, short_sandbox)
  ready = Queue.new
  worker = Thread.new do
    ready << :go
    long_sandbox.run(LONG_SCRIPT)
  end
  ready.pop
  elapsed = time_block { short_sandbox.run("nil") }
  worker.join
  elapsed
end

def record_6c(runner, baseline, contended)
  runner.results << { label: "6c-baseline-run-nil", seconds: baseline, mode: "concurrent" }
  runner.results << { label: "6c-contended-run-nil", seconds: contended, mode: "concurrent" }
  runner.results << { label: "6c-blocking-ratio", ratio: contended / baseline,
                      baseline_ms: baseline * 1000, contended_ms: contended * 1000,
                      mode: "concurrent" }
  puts format("6c-baseline      %<b>10.3f ms", b: baseline * 1000)
  puts format("6c-contended     %<c>10.3f ms (%<r>.1fx baseline)",
              c: contended * 1000, r: contended / baseline)
end

runner = Kobako::Bench::Runner.new("concurrent")
Kobako::Sandbox.new.run("nil") # warm process-wide caches

[1, 2, 4, 8].each do |count|
  measure_6a(runner, count)
  measure_6b(runner, count)
end
measure_6c(runner)

puts runner.write!
