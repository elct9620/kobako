# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #1 — Cold start latency.
#
#   1a — Sandbox.new alone (steady-state warm Sandbox construction)
#   1b — Sandbox.new + first #eval("nil") (steady-state warm new +
#        first one-shot source invocation)
#   1c — The first Sandbox.new in the process (cold: pays wasmtime
#        Engine init and Module compile) versus the median of the
#        next 9 (warm: hits the shared Engine and per-path Module
#        cache documented in `ext/kobako/src/wasm/cache.rs`).
#        README.md claims this amortisation; 1c is the regression
#        guard for that claim. The warm rounds aggregate to a median
#        because a single sub-millisecond round is hostage to
#        machine transients (see the README noise section).

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "guest"
require "runner"

runner = Kobako::Bench::Runner.new("cold_start")

# Hoist the injected Guest Binary path out of the measured blocks so the
# KOBAKO_BENCH_WASM lookup never lands in the timer.
guest = Kobako::Bench::Guest.path

runner.one_shot("1c-sandbox-new-cold") { Kobako::Sandbox.new(wasm_path: guest) }
runner.one_shot_median("1c-sandbox-new-warm", rounds: 9) { Kobako::Sandbox.new(wasm_path: guest) }

runner.case("1a-sandbox-new") { Kobako::Sandbox.new(wasm_path: guest) }

# 1b constructs a fresh Sandbox per iteration, so the +sandbox+ to
# sample +usage+ from is only knowable after the block runs; expose
# it through a closure-local binding the runner can read once the
# measurement loop finishes. +Sandbox.new+ alone leaves +usage+ at
# the EMPTY sentinel, which is why 1a does not annotate.
last_sandbox = nil
runner.case("1b-sandbox-new+eval-nil") do
  last_sandbox = Kobako::Sandbox.new(wasm_path: guest)
  last_sandbox.eval("nil")
end
runner.annotate_usage!(last_sandbox)

puts runner.write!
