# frozen_string_literal: true

# Characterization benchmark (not in release gate) — measures the
# memory cost of running Sandboxes across two complementary lenses:
#
#   - External RSS via `ps -o rss=`. Captures what the host process
#     consumes in total — Engine + compiled Module + every Sandbox
#     instance + every retained capture buffer. The right granularity
#     for capacity planning ("how many tenants fit in one process?").
#   - Per-invocation `Sandbox#usage` (docs/behavior.md B-35). The
#     guest's `memory.grow` delta and the guest export's wall-clock
#     time are sampled directly off `sandbox.usage` after the
#     measured invocation, so the JSON now attributes growth to the
#     guest linear-memory layer instead of folding it into host
#     allocator noise. 7c / 7d in particular benefit: a regression
#     that grows guest memory for the stdout-overflow path would be
#     invisible at the RSS layer but immediate at `memory_peak`.
#
#   7a — Per-Sandbox RSS cost. Measure RSS at baseline, after the
#        first Sandbox (which absorbs Engine + Module load), and
#        after N=10/100/1000 total Sandboxes. No usage attribution —
#        7a never invokes the guest, so `sandbox.usage` would be the
#        EMPTY sentinel.
#   7b — Per-invocation RSS drift. Run #eval("nil") 10 000 times on
#        a single Sandbox; sample RSS every 1 000 invocations and
#        sample the last invocation's `usage` alongside the RSS
#        sample. Bounded sub-linear RSS drift is allocator page
#        retention and expected; `memory_peak` per nil-returning
#        eval should stay ~0 because the script doesn't grow linear
#        memory. A B-15 / B-19 per-invocation reset violation *at
#        the linear-memory layer* would now surface as nonzero
#        `memory_peak` per call — a signal RSS drift cannot isolate.
#   7c — Large-payload retention. Measure RSS before, while holding
#        a 512 KiB return value, and after GC. `usage.memory_peak`
#        from the same invocation directly reports how much the
#        guest's `memory.grow` had to allocate for the 512 KiB
#        String, making the RSS jump attributable to guest growth
#        rather than allocator slack.
#   7d — Near-cap stdout retention. Run a script that fills the
#        default 1 MiB stdout_limit, sample RSS while the host-side
#        capture buffer still holds the bytes, then drop the
#        Sandbox reference and re-sample after GC. `usage.memory_peak`
#        is expected to stay small (stdout flows through the WASI
#        pipe, not guest linear memory); a regression that grows
#        guest memory on the stdout-overflow path would show up
#        here even though RSS would only see allocator slack.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

# 7c payload size — kept well below MRB_STR_LENGTH_MAX (1 MiB; SPEC
# Invariant) so the guest can construct the String without raising.
PAYLOAD_BYTES = 512 * 1024

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
  # memory_limit: nil — see benchmark/mruby_eval.rb for the rationale.
  # The 7c large-payload return (512 KiB String) and the 7d stdout-fill
  # script (2 MiB written) would both exceed the default 1 MiB per-
  # invocation delta cap; this suite measures host-side RSS / allocator
  # behavior and intentionally keeps the cap path out of the hot loop.
  Kobako::Sandbox.new(memory_limit: nil).tap { |s| s.eval("nil") }
end

def record(runner, label, sandbox: nil, **fields)
  runner.results << { label: label, mode: "memory", **fields }
  runner.annotate_usage!(sandbox) if sandbox
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
    sandboxes << Kobako::Sandbox.new(memory_limit: nil) while sandboxes.size < target
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

def measure_invocation_drift(runner)
  sandbox = warm_sandbox
  baseline = gc_then_rss
  record(runner, "7b-rss-before-eval-loop", rss_kb: baseline)
  puts format("7b baseline (1 sandbox, warm): rss=%<r>d KB", r: baseline)
  10.times { |i| record_drift(runner, sandbox, baseline, (i + 1) * 1000) }
end

def record_drift(runner, sandbox, baseline_kb, iter)
  1000.times { sandbox.eval("nil") }
  rss_kb = gc_then_rss
  drift = rss_kb - baseline_kb
  record(runner, "7b-rss-after-#{iter}-evals",
         sandbox: sandbox,
         rss_kb: rss_kb, delta_from_baseline_kb: drift)
  puts format("7b after %<n>5d evals: rss=%<r>d KB (drift %<d>+d KB)",
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
  script = "\"x\" * #{PAYLOAD_BYTES}"
  result = sandbox.eval(script)
  during = sample_rss_kb
  # `payload_bytesize` reads `result` *after* the rss sample, which
  # is what keeps the 512 KiB String alive across `sample_rss_kb`.
  # Do not drop this field: rubocop's auto-correct previously
  # stripped the unused `result =` assignment and silently broke
  # the during-payload measurement.
  record(runner, "7c-rss-while-holding-return-value",
         sandbox: sandbox,
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

# ---- 7d: near-cap stdout retention --------------------------------------

# Attempt 2 MiB of stdout writes against the 1 MiB default cap.
# Guest puts does not raise on cap rejection — the WASI pipe drops
# bytes past the cap and the loop runs to completion. Mirrors
# benchmark/mruby_eval.rb 4f.
STDOUT_FILL_SCRIPT = <<~RUBY
  2048.times { puts "x" * 1023 }
RUBY

def measure_near_cap_stdout(runner)
  sandbox = warm_sandbox
  before = gc_then_rss
  record(runner, "7d-rss-before-near-cap-stdout", rss_kb: before)
  during = sample_during_near_cap(runner, sandbox, before)
  sandbox = nil # rubocop:disable Lint/UselessAssignment
  record_near_cap_retention(runner, before, during)
end

def sample_during_near_cap(runner, sandbox, before)
  sandbox.eval(STDOUT_FILL_SCRIPT)
  during = sample_rss_kb
  # Read sandbox.stdout *after* the rss sample so the captured 1 MiB
  # String stays alive across the measurement window — mirrors the
  # 7c pattern (rubocop's auto-correct will strip an "unused" read).
  bytes = sandbox.stdout.bytesize
  record(runner, "7d-rss-while-holding-near-cap-stdout",
         sandbox: sandbox,
         rss_kb: during,
         peak_delta_kb: during - before,
         stdout_bytesize: bytes,
         stdout_truncated: sandbox.stdout_truncated?)
  during
end

def record_near_cap_retention(runner, before, during)
  after = gc_then_rss
  record(runner, "7d-rss-after-near-cap-stdout-gc",
         rss_kb: after, retained_delta_kb: after - before)
  puts format("7d before=%<b>d KB, during=%<d>d KB (peak +%<p>d), after-gc=%<a>d KB (retained %<r>+d)",
              b: before, d: during, p: during - before, a: after, r: after - before)
end

# ---- driver -------------------------------------------------------------

runner = Kobako::Bench::Runner.new("memory")

sandboxes = measure_per_sandbox_cost(runner)
sandboxes.clear
GC.start

measure_invocation_drift(runner)
measure_large_payload(runner)
measure_near_cap_stdout(runner)

puts runner.write!
