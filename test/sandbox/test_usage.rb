# frozen_string_literal: true

require "test_helper"

# Layer 4 — End-to-end coverage for `Kobako::Sandbox#usage`
# ({docs/behavior.md B-35}[link:../../docs/behavior.md]).
#
# Drives the real mruby Guest Binary (`data/kobako.wasm`) so the
# `wall_time` and `memory_peak` readers exercise the same wasmtime path
# the production caps in B-01 / E-19 / E-20 ride on. The contract under
# test: `#usage` returns `Kobako::Usage::EMPTY` before any invocation
# and is overwritten on every one of the four outcome classes — value
# return, `Kobako::TrapError` (including the cap subclasses), `Kobako::
# SandboxError`, and `Kobako::ServiceError` — so a Host App can read
# the record from any rescue branch. `memory_peak` never exceeds the
# configured `memory_limit` even on the E-20 trap.
class TestSandboxUsage < Minitest::Test
  REAL_WASM = File.expand_path("../../data/kobako.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "data/kobako.wasm missing — run `bundle exec rake wasm:build`" unless File.exist?(REAL_WASM)
  end

  # B-35: a fresh Sandbox returns the pre-invocation sentinel, so Host
  # Apps that read `#usage` before any invocation get a stable value
  # rather than `nil` and never need a guard clause.
  def test_usage_is_empty_before_any_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_same Kobako::Usage::EMPTY, sandbox.usage,
                "pre-invocation #usage must be the EMPTY sentinel, not a freshly-allocated zero record"
    assert_equal 0.0, sandbox.usage.wall_time
    assert_equal 0,   sandbox.usage.memory_peak
  end

  # B-35: a successful `#eval` populates `wall_time` with a positive
  # value because the guest export call always takes nonzero time to
  # execute. `memory_peak` is intentionally not asserted here —
  # `1 + 1` may or may not trigger `memory.grow`, and the meaningful
  # bound (`>= 200_000` for an allocating script) is pinned by
  # `test_second_invocation_overwrites_usage_from_first` below.
  def test_eval_success_populates_wall_time
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    sandbox.eval("1 + 1")

    assert_operator sandbox.usage.wall_time, :>, 0.0,
                    "wall_time must be positive after a successful invocation — " \
                    "the bracket covers the guest export call"
  end

  # B-35: `#run` shares the same usage path as `#eval`. Pin both verbs
  # so a regression that only wires one is caught.
  def test_run_success_populates_wall_time
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Entry = ->(*_args, **_kw) { 42 }", name: :Entry)

    assert_equal 42, sandbox.run(:Entry)
    assert_operator sandbox.usage.wall_time, :>, 0.0
  end

  # B-35: subsequent invocations overwrite `#usage` rather than
  # accumulate, mirroring `#stdout` / `#stderr` semantics. A script
  # that allocates ~200 KiB must report a `memory_peak` larger than
  # the no-allocation baseline of the prior invocation.
  def test_second_invocation_overwrites_usage_from_first
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    sandbox.eval("1 + 1")
    sandbox.eval('"x" * 200_000')

    assert_operator sandbox.usage.wall_time, :>, 0.0,
                    "second invocation must produce its own wall_time, not stale state from the first"
    assert_operator sandbox.usage.memory_peak, :>=, 200_000,
                    "an allocation of ~200 KiB must register through memory_growing past the entry-time baseline"
  end

  # B-35: the usage record is populated even when the invocation
  # terminates via a `TimeoutError` trap. A Host App reading `#usage`
  # in the rescue branch must see a real measurement so it can decide
  # whether the script ran long because of CPU work or host-side
  # Service callback time.
  def test_timeout_trap_path_still_populates_usage
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    assert_raises(Kobako::TimeoutError) { sandbox.eval("loop { }") }

    assert_operator sandbox.usage.wall_time, :>=, 0.2,
                    "wall_time after TimeoutError must reflect at least the configured timeout"
    refute_same Kobako::Usage::EMPTY, sandbox.usage,
                "the ensure block must overwrite EMPTY with the real measurement even on the trap path"
  end

  # B-35: on `MemoryLimitError`, `memory_peak` reports the last
  # accepted grow rather than the rejected `desired` — so the reading
  # never exceeds `memory_limit`. Without this guarantee a Host App
  # reading the failure would see a budget violation in the
  # observability record itself.
  def test_memory_limit_trap_caps_memory_peak_at_memory_limit
    memory_limit = 2 << 20 # 2 MiB
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: memory_limit)

    assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('a = []; 200.times { a << ("x" * 100_000) }; nil')
    end

    assert_operator sandbox.usage.memory_peak, :<=, memory_limit,
                    "memory_peak must never exceed memory_limit; " \
                    "rejected desired values are not promoted into the high-water"
    assert_operator sandbox.usage.wall_time, :>, 0.0
  end

  # B-35: a guest-side raise propagates out as `Kobako::SandboxError`
  # via the Panic envelope path (E-04). The usage record is still
  # populated because `invoke!` reads it in an `ensure` block — without
  # this guarantee, a Host App rescuing a runtime guest error would
  # have no signal about how much of the budget the failing invocation
  # consumed.
  def test_sandbox_error_path_still_populates_usage
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_raises(Kobako::SandboxError) { sandbox.eval('raise "boom"') }

    refute_same Kobako::Usage::EMPTY, sandbox.usage,
                "ensure block must overwrite EMPTY on the SandboxError outcome path too"
    assert_operator sandbox.usage.wall_time, :>, 0.0
  end

  # B-35: an unrescued Service-call failure surfaces as
  # `Kobako::ServiceError` (E-13). Same guarantee as the SandboxError
  # path — pinning all four outcome classes (success, TrapError,
  # SandboxError, ServiceError) prevents a future refactor that moves
  # `read_usage!` out of `invoke!`'s ensure block from silently
  # breaking subset of the contract.
  def test_service_error_path_still_populates_usage
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Log).bind(:Sink, ->(_msg) { raise "capability denied" })

    assert_raises(Kobako::ServiceError) { sandbox.eval('Log::Sink.call("x")') }

    refute_same Kobako::Usage::EMPTY, sandbox.usage,
                "ensure block must overwrite EMPTY on the ServiceError outcome path too"
    assert_operator sandbox.usage.wall_time, :>, 0.0
  end
end
