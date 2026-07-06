# frozen_string_literal: true

require "test_helper"

# Differential parity — error taxonomy (SPEC.md E-04, E-05, E-19,
# E-20; E-01 pending): each failure origin must reach the same
# neutral status and guest class through both frontends.
class TestParityErrors < Parity::Case
  # SPEC.md E-04: an uncaught guest exception is a sandbox-origin
  # failure carrying the guest class.
  def test_uncaught_guest_exception
    assert_parity Parity::Scenario.new(
      name: "uncaught-raise", anchors: %w[E-04],
      invocations: [
        { verb: "eval", source: 'raise "boom"' },
        { verb: "eval", source: 'class MyFault < StandardError; end; raise MyFault, "typed"' }
      ]
    )
  end

  # SPEC.md E-05: a source that fails to compile is a sandbox-origin
  # failure, not a trap.
  def test_compile_failure
    assert_parity Parity::Scenario.new(
      name: "compile-failure", anchors: %w[E-05],
      invocations: [{ verb: "eval", source: "def broken(" }]
    )
  end

  # SPEC.md B-01 / E-19: the wall-clock cap interrupts an infinite
  # loop with the timeout status on both sides.
  def test_timeout_cap
    assert_parity Parity::Scenario.new(
      name: "timeout-cap", anchors: %w[B-01 E-19],
      options: { timeout_ms: 300 },
      invocations: [{ verb: "eval", source: "loop { }" }]
    )
  end

  # SPEC.md B-01 / E-20: the linear-memory cap traps runaway
  # allocation with the memory-limit status on both sides.
  def test_memory_limit_cap
    assert_parity Parity::Scenario.new(
      name: "memory-limit-cap", anchors: %w[E-20],
      options: { memory_limit: 2 << 20, timeout_ms: 5000 },
      invocations: [{ verb: "eval", source: 'a = []; 200.times { a << ("x" * 100_000) }; nil' }]
    )
  end

  # SPEC.md E-01: a raw engine trap (not a cap) has no deterministic
  # pure-mruby trigger — the guest turns deep recursion into its own
  # SystemStackError before wasm faults.
  def test_engine_trap_pending
    skip "pending a deterministic guest trap trigger (E-01)"
  end
end
