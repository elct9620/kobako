# frozen_string_literal: true

require "test_helper"

# Layer 4 — End-to-end coverage for `Kobako::Execution#usage`
# ({docs/behavior/lifecycle.md B-35}[link:../../docs/behavior/lifecycle.md]).
#
# Drives the real mruby Guest Binary (`data/kobako.wasm`) so the
# `wall_time` and `memory_peak` readers exercise the same wasmtime path
# the production caps in B-01 / E-19 / E-20 ride on. The contract under
# test: `#eval` / `#run` return a `Kobako::Execution` whose `#usage` is
# populated on every one of the four outcome classes — value return,
# `Kobako::TrapError` (including the cap subclasses), `Kobako::SandboxError`,
# and `Kobako::ServiceError`. A failed run raises an error carrying the same
# Execution, so a Host App reads `#usage` off the rescue branch exactly as a
# successful caller reads it off the return value. `memory_peak` never
# exceeds the configured `memory_limit` even on the E-20 trap.
class TestSandboxUsage < Minitest::Test
  include E2eGuestHelper

  # B-35: a successful `#eval` populates `wall_time` with a positive
  # value because the guest export call always takes nonzero time to
  # execute. `memory_peak` is intentionally not asserted here —
  # `1 + 1` may or may not trigger `memory.grow`, and the meaningful
  # bound (`>= 200_000` for an allocating script) is pinned by
  # `test_allocating_eval_reports_memory_peak` below.
  def test_eval_success_populates_wall_time
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    usage = sandbox.eval("1 + 1").usage

    assert_operator usage.wall_time, :>, 0.0,
                    "wall_time must be positive after a successful invocation — " \
                    "the bracket covers the guest export call"
    # Pin the numeric types the ext binding carries back on the Snapshot: the
    # assertions above pass for either type (0.0 == 0, :> on any numeric), so
    # a Float→Integer drift in the ext binding would slip through without these.
    assert_kind_of Float, usage.wall_time,
                   "a successful invocation's Execution#usage must report wall_time as Float seconds"
    assert_kind_of Integer, usage.memory_peak,
                   "a successful invocation's Execution#usage must report memory_peak as Integer bytes"
  end

  # B-35: `#run` shares the same usage path as `#eval`. Pin both verbs
  # so a regression that only wires one is caught.
  def test_run_success_populates_wall_time
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Entry = ->(*_args, **_kw) { 42 }", name: :Entry)

    execution = sandbox.run(:Entry)

    assert_equal 42, execution.value
    assert_operator execution.usage.wall_time, :>, 0.0
  end

  # B-35: each invocation's Execution carries its own usage. A script
  # that allocates ~200 KiB must report a `memory_peak` past the
  # no-allocation baseline through `memory_growing`.
  def test_allocating_eval_reports_memory_peak
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    usage = sandbox.eval('"x" * 200_000').usage

    assert_operator usage.wall_time, :>, 0.0,
                    "an allocating invocation must report its own wall_time"
    assert_operator usage.memory_peak, :>=, 200_000,
                    "an allocation of ~200 KiB must register through memory_growing past the entry-time baseline"
  end

  # B-35: the usage record is populated even when the invocation
  # terminates via a `TimeoutError` trap. A Host App reading `#usage`
  # off the carried Execution in the rescue branch must see a real
  # measurement so it can decide whether the script ran long because of
  # CPU work or host-side Service callback time.
  def test_timeout_trap_path_still_populates_usage
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    error = assert_raises(Kobako::TimeoutError) { sandbox.eval("loop { }") }

    assert_operator error.execution.usage.wall_time, :>=, 0.2,
                    "wall_time after TimeoutError must reflect at least the configured timeout"
    refute_same Kobako::Usage::EMPTY, error.execution.usage,
                "a timed-out invocation's carried Execution must report the real usage, not the pre-run sentinel"
  end

  # B-35: on `MemoryLimitError`, `memory_peak` reports the last
  # accepted grow rather than the rejected `desired` — so the reading
  # never exceeds `memory_limit`. Without this guarantee a Host App
  # reading the failure would see a budget violation in the
  # observability record itself.
  def test_memory_limit_trap_caps_memory_peak_at_memory_limit
    memory_limit = 2 << 20 # 2 MiB
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: memory_limit)

    error = assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('a = []; 200.times { a << ("x" * 100_000) }; nil')
    end

    assert_operator error.execution.usage.memory_peak, :<=, memory_limit,
                    "memory_peak must never exceed memory_limit; " \
                    "rejected desired values are not promoted into the high-water"
    assert_operator error.execution.usage.wall_time, :>, 0.0
  end

  # B-35: a guest-side raise propagates out as `Kobako::SandboxError`
  # via the Panic envelope path (E-04). Its carried Execution still holds
  # the run's usage, so a Host App rescuing a runtime guest error can see
  # how much of the budget the failing invocation consumed.
  def test_sandbox_error_path_still_populates_usage
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    error = assert_raises(Kobako::SandboxError) { sandbox.eval('raise "boom"') }

    refute_same Kobako::Usage::EMPTY, error.execution.usage,
                "a SandboxError's carried Execution must report the real usage, not the pre-run sentinel"
    assert_operator error.execution.usage.wall_time, :>, 0.0
  end

  # B-35: an unrescued Service-call failure surfaces as
  # `Kobako::ServiceError` (E-13). Same guarantee as the SandboxError
  # path — pinning all four outcome classes (success, TrapError,
  # SandboxError, ServiceError) proves usage rides the carried Execution
  # on every outcome the guest reached, not only the value-return one.
  def test_service_error_path_still_populates_usage
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Log::Sink", ->(_msg) { raise "capability denied" })

    error = assert_raises(Kobako::ServiceError) { sandbox.eval('Log::Sink.call("x")') }

    refute_same Kobako::Usage::EMPTY, error.execution.usage,
                "a ServiceError's carried Execution must report the real usage, not the pre-run sentinel"
    assert_operator error.execution.usage.wall_time, :>, 0.0
  end
end
