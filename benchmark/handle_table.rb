# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #5 — Handle allocation and release
# throughput. SPEC: "HandleTable internal dictionary and counter
# performance."
#
#   5a — Cumulative cost of allocating N entries from an empty
#        HandleTable. Each iteration builds a fresh table and runs
#        N #alloc calls, so the ips number divided by N approximates
#        the average per-alloc cost up to size N. This is bulk
#        throughput, not marginal cost at size N — for the latter
#        see 5b.
#   5b — Marginal per-alloc cost at table-size waypoints
#        (1K / 10K / 100K / 1M). The table is grown OUTSIDE the
#        timer; only the 1000 measured allocs land inside one_shot.
#        Flat numbers here mean the underlying Hash stays O(1) as
#        it grows; SPEC's "approach the 2^31 − 1 cap" intent
#        reframed as "does the dictionary degrade." The cap guard
#        itself is constant-time and not iterated.
#   5c — Warm Sandbox#eval("nil") round-trip cost measured WHILE the
#        1 M-entry HandleTable grown by 5b is still alive in the same
#        Ruby process. The clean per-invocation roundtrip number lives
#        at cold_start.rb 1b (~275 µs); 5c is positioned as the
#        GC-pressure regression guard — every measured invocation
#        allocates capture-buffer Strings under heavy heap pressure
#        from the retained 1 M-entry table, so a regression specific
#        to "invocation becomes more GC-sensitive when the process
#        already holds a large HandleTable" is detectable here even
#        though 1b would miss it. The B-15 / B-19 per-invocation
#        reset itself is constant-time and not what this case
#        isolates.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("handle_table")

{ "0" => 0, "100" => 100, "10_000" => 10_000, "100_000" => 100_000 }.each do |label, prefill|
  runner.case("5a-alloc-#{label}-from-empty") do
    table = Kobako::RPC::HandleTable.new
    prefill.times { table.alloc(Object.new) }
    table.alloc(Object.new)
  end
end

batch_table = Kobako::RPC::HandleTable.new
batch_obj = Object.new
[1_000, 10_000, 100_000, 1_000_000].each do |target|
  (target - batch_table.size - 1000).times { batch_table.alloc(batch_obj) }
  runner.one_shot("5b-alloc-1000-at-size-#{target}") do
    1000.times { batch_table.alloc(batch_obj) }
  end
end

# memory_limit: nil — see benchmark/mruby_eval.rb for rationale.
sandbox = Kobako::Sandbox.new(memory_limit: nil)
sandbox.eval("nil") # warm

# Runs after the 5b loop has retained `batch_table` (1 M Handles) at
# module scope for the rest of the process — the GC pressure that
# distinguishes this case from cold_start.rb 1b.
runner.case_with_usage("5c-warm-eval-nil-under-gc-pressure", sandbox) { sandbox.eval("nil") }

puts runner.write!
