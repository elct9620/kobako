# frozen_string_literal: true

# Characterization benchmark (not in release gate) — measures
# behaviour under multi-Thread Sandbox usage, the typical Sidekiq /
# Puma cluster shape. SPEC.md does not currently specify a
# concurrency contract; this benchmark observes the status quo so
# future ext/ changes (e.g. introducing rb_thread_call_without_gvl)
# can be compared before/after.
#
#   7a — N Threads each owning a Sandbox, running #eval in parallel.
#        Under Ruby's GVL with no rb_thread_call_without_gvl call
#        in ext/, total throughput is expected to stay close to flat
#        across N — modest scaling can appear because Ruby-side
#        setup before each #eval (preamble pack, buffer init) can
#        overlap across threads even while wasm execution is
#        serialised.
#   7b — N Threads each calling Sandbox.new cold. Measures mutex
#        contention on the shared MODULE_CACHE in
#        ext/kobako/src/wasm/cache.rs.
#   7c — Concurrent contention overhead: one Thread runs a long
#        #eval, a second Thread tries to start its own #eval("nil").
#        The worker signals readiness via a host-bound Service
#        (Sync::Ready) from inside wasm, so the measurement is
#        provably taken after the worker has entered the wasm
#        execution path — eliminating the obvious race in a naive
#        `Queue << :go` before run pattern. The 2-3x ratio we
#        observe is NOT "main is blocked for the full long script"
#        — Queue#<< on the host side itself releases the GVL, so
#        main interleaves almost immediately. The number captures
#        the realistic GVL-handoff overhead under any workload
#        whose host-side sync touches a Ruby primitive that yields.

$LOAD_PATH.unshift File.expand_path("../../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("../support", __dir__)

require "kobako"
require "guest"
require "runner"

# The injected Guest Binary path, resolved once outside every measured
# block so the KOBAKO_BENCH_WASM lookup never lands in the timer.
GUEST = Kobako::Bench::Guest.path

OPS_PER_THREAD_6A = 50

# Synchronized long script: the first guest expression calls into
# the host-side Sync::Ready Service, which pushes onto the ready
# Queue. By the time `Sync::Ready.call` returns inside wasm, the
# worker Thread is provably past Sandbox#eval setup and inside the
# wasm execution path.
SYNCED_LONG_SCRIPT = <<~RUBY
  Sync::Ready.call
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

def measure_7a(runner, count)
  sandboxes = Array.new(count) { Kobako::Sandbox.new(wasm_path: GUEST) }
  sandboxes.each { |s| s.eval("nil") }
  elapsed = time_block { parallel_join(count) { |i| OPS_PER_THREAD_6A.times { sandboxes[i].eval("nil") } } }
  total = count * OPS_PER_THREAD_6A
  runner.results << { label: "7a-threads-#{count}", seconds: elapsed,
                      ops: total, ops_per_sec: total / elapsed, mode: "concurrent" }
  puts format("7a-threads-%<n>-3d %<rate>12.1f ops/s", n: count, rate: total / elapsed)
end

def measure_7b(runner, count)
  elapsed = time_block { parallel_join(count) { Kobako::Sandbox.new(wasm_path: GUEST) } }
  runner.results << { label: "7b-new-#{count}", seconds: elapsed,
                      constructions: count, per_construction_seconds: elapsed / count,
                      mode: "concurrent" }
  puts format("7b-new-%<n>-3d %<sec>12.3f ms (%<per>.3f ms each)",
              n: count, sec: elapsed * 1000, per: (elapsed / count) * 1000)
end

def measure_7c(runner)
  ready = Queue.new
  short = Kobako::Sandbox.new(wasm_path: GUEST)
  long = build_synced_long_sandbox(ready)
  short.eval("nil")
  long.eval("nil") # warm — does not trip Sync::Ready
  baseline = time_block { short.eval("nil") }
  contended = run_under_contention(long, short, ready)
  record_7c(runner, baseline, contended)
end

def build_synced_long_sandbox(ready)
  Kobako::Sandbox.new(wasm_path: GUEST).tap do |s|
    s.bind("Sync::Ready", lambda {
      ready << :go
      nil
    })
  end
end

def run_under_contention(long_sandbox, short_sandbox, ready)
  worker = Thread.new { long_sandbox.eval(SYNCED_LONG_SCRIPT) }
  ready.pop # blocks until Sync::Ready.call returns inside wasm
  elapsed = time_block { short_sandbox.eval("nil") }
  worker.join
  elapsed
end

def record_7c(runner, baseline, contended)
  runner.results << { label: "7c-baseline-eval-nil", seconds: baseline, mode: "concurrent" }
  runner.results << { label: "7c-contended-eval-nil", seconds: contended, mode: "concurrent" }
  runner.results << { label: "7c-blocking-ratio", ratio: contended / baseline,
                      baseline_ms: baseline * 1000, contended_ms: contended * 1000,
                      mode: "concurrent" }
  puts format("7c-baseline      %<b>10.3f ms", b: baseline * 1000)
  puts format("7c-contended     %<c>10.3f ms (%<r>.1fx baseline)",
              c: contended * 1000, r: contended / baseline)
end

runner = Kobako::Bench::Runner.new("concurrent")
Kobako::Sandbox.new(wasm_path: GUEST).eval("nil") # warm process-wide caches

[1, 2, 4, 8].each do |count|
  measure_7a(runner, count)
  measure_7b(runner, count)
end
measure_7c(runner)

puts runner.write!
