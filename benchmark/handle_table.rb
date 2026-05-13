# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #5 — Handle allocation and release
# throughput. SPEC: "HandleTable internal dictionary and counter
# performance."
#
#   5a — HandleTable#alloc throughput at varying entry counts to
#        expose any non-O(1) scaling in the underlying Hash
#   5b — alloc throughput sampled at table-size waypoints
#        (1K / 10K / 100K / 1M) — the SPEC #5b "approach the cap"
#        intent reframed as "does the dictionary stay flat as it
#        grows." The 2^31 − 1 cap itself is a constant-time guard,
#        not iterated.
#   5c — Per-#run reset cost: 100 consecutive Sandbox#run("nil")
#        calls. Each #run resets HandleTable per SPEC B-15 / B-19.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("handle_table")

# 5a — fresh table per iteration; pre-allocate N entries then time
# the single next alloc call so the measurement isolates the per-
# alloc cost at table size N.
{ "0" => 0, "100" => 100, "10_000" => 10_000, "100_000" => 100_000 }.each do |label, prefill|
  runner.case("5a-alloc-at-size-#{label}") do
    table = Kobako::Registry::HandleTable.new
    prefill.times { table.alloc(Object.new) }
    table.alloc(Object.new)
  end
end

# 5b — single growing table; measure batches of 1000 alloc calls
# at waypoints. The table is grown to each waypoint OUTSIDE the
# timer; only the 1000 measured allocs land inside one_shot.
batch_table = Kobako::Registry::HandleTable.new
batch_obj = Object.new
[1_000, 10_000, 100_000, 1_000_000].each do |target|
  (target - batch_table.size - 1000).times { batch_table.alloc(batch_obj) }
  runner.one_shot("5b-alloc-1000-at-size-#{target}") do
    1000.times { batch_table.alloc(batch_obj) }
  end
end

# 5c — per-#run reset cost. Each Sandbox#run resets the
# HandleTable (B-15 counter to 1, B-19 entries cleared). The
# Sandbox is reused so engine/module caches and Wasm instance are
# warm; the reset call dominates over any other per-#run setup
# growth.
sandbox = Kobako::Sandbox.new
sandbox.run("nil") # warm

runner.case("5c-per-run-reset-nil") { sandbox.run("nil") }

puts runner.write!
