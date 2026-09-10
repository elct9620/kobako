# frozen_string_literal: true

require "test_helper"

# Differential parity — error taxonomy: each failure origin must reach
# the same neutral status and guest class through both frontends.
class TestParityErrors < Parity::Case
  # @behavior OC-024
  # Both an anonymous raise and one of the guest's own class are run,
  # since a frontend could carry the class for one and not the other.
  def test_uncaught_guest_exception
    assert_parity Parity::Scenario.new(
      name: "uncaught-raise",
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
      name: "compile-failure",
      invocations: [{ verb: "eval", source: "def broken(" }]
    )
  end

  # @behavior OC-026
  def test_timeout_cap
    assert_parity Parity::Scenario.new(
      name: "timeout-cap",
      options: { timeout_ms: 300 },
      invocations: [{ verb: "eval", source: "loop { }" }]
    )
  end

  # @behavior OC-027
  def test_memory_limit_cap
    assert_parity Parity::Scenario.new(
      name: "memory-limit-cap",
      options: { memory_limit: 2 << 20, timeout_ms: 5000 },
      invocations: [{ verb: "eval", source: 'a = []; 200.times { a << ("x" * 100_000) }; nil' }]
    )
  end

  # A raw engine trap (not a cap) has no deterministic pure-mruby trigger:
  # the guest turns deep recursion into its own SystemStackError before
  # wasm faults, and the one live path (a host exception escaping the
  # dispatch callback) is frontend-specific by nature. The Ruby side is
  # pinned in test/e2e/test_capability_exception_safety.rb, trap-kind
  # routing in the driver's classify_trap tests.
  def test_engine_trap_pending
    skip "pending a deterministic guest trap trigger"
  end
end
