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
#   5c — Warm Sandbox#run("nil") round-trip cost. Every #run
#        triggers HandleTable#reset (B-15 counter → 1; B-19 entries
#        cleared) plus stdout/stderr buffer clears and the wasi
#        pipe setup → __kobako_run → drain → decode chain; reset is
#        only one component of the measured cost.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("handle_table")

{ "0" => 0, "100" => 100, "10_000" => 10_000, "100_000" => 100_000 }.each do |label, prefill|
  runner.case("5a-alloc-#{label}-from-empty") do
    table = Kobako::RPC::Server::HandleTable.new
    prefill.times { table.alloc(Object.new) }
    table.alloc(Object.new)
  end
end

batch_table = Kobako::RPC::Server::HandleTable.new
batch_obj = Object.new
[1_000, 10_000, 100_000, 1_000_000].each do |target|
  (target - batch_table.size - 1000).times { batch_table.alloc(batch_obj) }
  runner.one_shot("5b-alloc-1000-at-size-#{target}") do
    1000.times { batch_table.alloc(batch_obj) }
  end
end

sandbox = Kobako::Sandbox.new
sandbox.run("nil") # warm

runner.case("5c-warm-run-nil-roundtrip") { sandbox.run("nil") }

puts runner.write!
