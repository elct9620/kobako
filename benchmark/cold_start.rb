# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #1 — Cold start latency.
#
#   1a — Sandbox.new alone (steady-state warm Sandbox construction)
#   1b — Sandbox.new + first #run("nil") (steady-state warm new + run)
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

runner.case("1b-sandbox-new+run-nil") do
  Kobako::Sandbox.new.run("nil")
end

puts runner.write!
