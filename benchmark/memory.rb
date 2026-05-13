# frozen_string_literal: true

# Characterization benchmark (not in release gate) — measures the
# *external* memory cost of running Sandboxes. We never look inside
# the Sandbox (no Wasm memory size, no mruby heap inspection); we
# only observe what the host process consumes via RSS. This is the
# right granularity for capacity planning ("how many tenants fit in
# one process?") without violating SPEC's Non-Goal of per-#run
# instrumentation.
#
#   7a — Per-Sandbox RSS cost. Measure RSS at baseline, after the
#        first Sandbox (which absorbs Engine + Module load), and
#        after N=10/100/1000 total Sandboxes. The delta divided by
#        (N - 1) approximates per-additional-Sandbox cost.
#   7b — Per-#run RSS drift. Run #run("nil") 10 000 times on a
#        single Sandbox; sample RSS every 1 000 runs. A flat curve
#        verifies SPEC B-15 / B-19 per-run reset is not leaking.
#   7c — Large-payload retention. Measure RSS before, while holding
#        the 512 KiB return value, and after GC. A retained delta
#        of zero verifies the peak is released.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

# RSS in KB via `ps -o rss=` — macOS and Linux both report in 1024-byte
# blocks. Reading our own PID, so no quoting concern.
def sample_rss_kb
  Integer(`ps -o rss= -p #{Process.pid}`.strip)
end

def gc_then_rss
  GC.start
  sample_rss_kb
end

def warm_sandbox
  Kobako::Sandbox.new.tap { |s| s.run("nil") }
end

def record(runner, label, **fields)
  runner.results << { label: label, mode: "memory", **fields }
end

# ---- 7a: per-Sandbox RSS cost -------------------------------------------

def measure_per_sandbox_cost(runner)
  baseline = gc_then_rss
  record(runner, "7a-rss-baseline", rss_kb: baseline)
  first = warm_sandbox
  after_first = gc_then_rss
  record_first_sandbox(runner, baseline, after_first)
  grow_sandboxes(runner, [first], after_first)
end

def record_first_sandbox(runner, baseline, after_first)
  record(runner, "7a-rss-after-1-sandbox",
         rss_kb: after_first, delta_from_baseline_kb: after_first - baseline)
  puts format("baseline=%<b>d KB, after-first=%<a>d KB (engine+module+1 sandbox = +%<d>d KB)",
              b: baseline, a: after_first, d: after_first - baseline)
end

def grow_sandboxes(runner, sandboxes, after_first)
  [10, 100, 1000].each do |target|
    sandboxes << Kobako::Sandbox.new while sandboxes.size < target
    record_growth(runner, target, gc_then_rss, after_first)
  end
  sandboxes
end

def record_growth(runner, target, rss_kb, after_first_kb)
  delta = rss_kb - after_first_kb
  per = delta.to_f / (target - 1)
  record(runner, "7a-rss-after-#{target}-sandboxes",
         rss_kb: rss_kb, delta_from_first_kb: delta, per_additional_sandbox_kb: per.round(1))
  puts format("after %<n>4d sandboxes: rss=%<r>d KB, per-additional=%<p>.1f KB",
              n: target, r: rss_kb, p: per)
end

# ---- 7b: per-#run RSS drift ---------------------------------------------

def measure_run_drift(runner)
  sandbox = warm_sandbox
  baseline = gc_then_rss
  record(runner, "7b-rss-before-run-loop", rss_kb: baseline)
  puts format("7b baseline (1 sandbox, warm): rss=%<r>d KB", r: baseline)
  10.times { |i| record_drift(runner, sandbox, baseline, (i + 1) * 1000) }
end

def record_drift(runner, sandbox, baseline_kb, iter)
  1000.times { sandbox.run("nil") }
  rss_kb = gc_then_rss
  drift = rss_kb - baseline_kb
  record(runner, "7b-rss-after-#{iter}-runs",
         rss_kb: rss_kb, delta_from_baseline_kb: drift)
  puts format("7b after %<n>5d runs: rss=%<r>d KB (drift %<d>+d KB)",
              n: iter, r: rss_kb, d: drift)
end

# ---- 7c: large-payload retention ----------------------------------------

def measure_large_payload(runner)
  sandbox = warm_sandbox
  before = gc_then_rss
  during = sample_during_payload(runner, sandbox, before)
  record_retention(runner, before, during)
end

def sample_during_payload(runner, sandbox, before)
  record(runner, "7c-rss-before-512kib-return", rss_kb: before)
  result = sandbox.run('"x" * 524288')
  during = sample_rss_kb
  record(runner, "7c-rss-while-holding-return-value",
         rss_kb: during,
         peak_delta_kb: during - before,
         payload_bytesize: result.bytesize)
  during
end

def record_retention(runner, before, during)
  after = gc_then_rss
  record(runner, "7c-rss-after-gc",
         rss_kb: after, retained_delta_kb: after - before)
  puts format("7c before=%<b>d KB, during=%<d>d KB (peak +%<p>d), after-gc=%<a>d KB (retained %<r>+d)",
              b: before, d: during, p: during - before, a: after, r: after - before)
end

# ---- driver -------------------------------------------------------------

runner = Kobako::Bench::Runner.new("memory")

sandboxes = measure_per_sandbox_cost(runner)
sandboxes.clear
GC.start

measure_run_drift(runner)
measure_large_payload(runner)

puts runner.write!
