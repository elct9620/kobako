# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #1 — Cold start latency.
#
#   1a — Sandbox.new alone (steady-state warm Sandbox construction)
#   1b — Sandbox.new + first #eval("nil") (steady-state warm new +
#        first one-shot source invocation)
#   1c — First 10 Sandbox.new calls in the process, individually
#        timed. The very first call pays the wasmtime Engine init
#        and Module compile cost; subsequent calls hit the shared
#        Engine and per-path Module cache documented in
#        `ext/kobako/src/wasm/cache.rs`. README.md claims this
#        amortisation; 1c is the regression guard for that claim.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("cold_start")

10.times do |i|
  runner.one_shot("1c-sandbox-new-#{i + 1}") { Kobako::Sandbox.new }
end

runner.case("1a-sandbox-new") { Kobako::Sandbox.new }

# 1b constructs a fresh Sandbox per iteration, so the +sandbox+ to
# sample +usage+ from is only knowable after the block runs; expose
# it through a closure-local binding the runner can read once the
# measurement loop finishes. +Sandbox.new+ alone leaves +usage+ at
# the EMPTY sentinel, which is why 1a does not annotate.
last_sandbox = nil
runner.case("1b-sandbox-new+eval-nil") do
  last_sandbox = Kobako::Sandbox.new
  last_sandbox.eval("nil")
end
runner.annotate_usage!(last_sandbox)

puts runner.write!
