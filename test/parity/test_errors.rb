# frozen_string_literal: true

require "test_helper"

# Differential parity — error taxonomy (SPEC.md E-04, E-05, E-19,
# E-20; E-01 pending): each failure origin must reach the same
# neutral status and guest class through both frontends.
class TestParityErrors < Parity::Case
  # @behavior OC-024
  # Both an anonymous raise and one of the guest's own class are run,
  # since a frontend could carry the class for one and not the other.
  def test_uncaught_guest_exception
    assert_parity Parity::Scenario.new(
      name: "uncaught-raise", anchors: %w[E-04],
      invocations: [
        { verb: "eval", source: 'raise "boom"' },
        { verb: "eval", source: 'class MyFault < StandardError; end; raise MyFault, "typed"' }
      ]
    )
  end

  # @behavior OC-025
  # A frontend attributing this as a trap would tell its Host App to
  # discard a Sandbox that never ran anything.
  def test_compile_failure
    assert_parity Parity::Scenario.new(
      name: "compile-failure", anchors: %w[E-05],
      invocations: [{ verb: "eval", source: "def broken(" }]
    )
  end

  # @behavior OC-026
  def test_timeout_cap
    assert_parity Parity::Scenario.new(
      name: "timeout-cap", anchors: %w[B-01 E-19],
      options: { timeout_ms: 300 },
      invocations: [{ verb: "eval", source: "loop { }" }]
    )
  end

  # @behavior OC-027
  def test_memory_limit_cap
    assert_parity Parity::Scenario.new(
      name: "memory-limit-cap", anchors: %w[E-20],
      options: { memory_limit: 2 << 20, timeout_ms: 5000 },
      invocations: [{ verb: "eval", source: 'a = []; 200.times { a << ("x" * 100_000) }; nil' }]
    )
  end

  # SPEC.md E-01: a raw engine trap (not a cap) has no deterministic
  # pure-mruby trigger — the guest turns deep recursion into its own
  # SystemStackError before wasm faults, and the one live E-01 path (a
  # host exception escaping the dispatch callback) is frontend-specific
  # by nature. Ruby-side E-01 behavior is pinned end-to-end in
  # test/e2e/test_capability_exception_safety.rb; trap-kind routing is
  # unit-pinned in the driver's classify_trap tests.
  def test_engine_trap_pending
    skip "pending a deterministic guest trap trigger (E-01)"
  end
end
