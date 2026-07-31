# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #1 — Cold start latency.
#
#   1a — Sandbox.new alone (steady-state warm Sandbox construction)
#   1b — Sandbox.new + first #eval("nil") (steady-state warm new +
#        first one-shot source invocation)
#   1c — The first Sandbox.new in the process (cold: pays wasmtime
#        Engine init and Module compile) versus the median of the
#        next 9 (warm: hits the shared Engine and per-path Module
#        cache documented in `crates/kobako-wasmtime/src/cache.rs`).
#        README.md claims this amortisation; 1c is the regression
#        guard for that claim. The warm rounds aggregate to a median
#        because a single sub-millisecond round is hostage to
#        machine transients. Recording seconds keeps the pair out of
#        the gate: its worth is the ratio between its halves, which
#        tolerates a resolution a per-invocation commitment would not.

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

# 1b samples its guest budget rather than observing it once: the row is
# gated on +wall_time+, and a single observation carries no dispersion for
# the noise band to read, leaving the +10% floor as the only bar on a row
# that has swung 22-28 µs across captures. Constructing a fresh Sandbox
# each iteration is what the row measures, so the sampling loop repeats
# it. +Sandbox.new+ alone runs no invocation and yields no +Execution+,
# which is why 1a carries no usage at all.
runner.case_with_usage("1b-sandbox-new+eval-nil") do
  Kobako::Sandbox.new(wasm_path: guest).eval("nil")
end

puts runner.write!
