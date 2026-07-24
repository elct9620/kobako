# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the Kobako::Execution value object (B-61). #eval / #run
# return one on success and carry the same frozen object on a raised error's
# #execution, and #failed? tells the two apart. Driven through real mruby.
class TestE2EExecution < Minitest::Test
  include E2eGuestHelper

  # B-61: #failed? tells a successful run whose value was legitimately nil from
  # a failed run — both have a nil #value, so the object alone must stay
  # distinguishable without knowing whether it was returned or rescued.
  def test_failed_disambiguates_a_nil_value_success_from_a_failure
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    success = sandbox.eval("nil")
    failure = assert_raises(Kobako::SandboxError) { sandbox.eval('raise "boom"') }.execution

    assert_nil success.value, "B-61: a run whose last expression is nil has a nil #value"
    assert_nil failure.value, "B-61: a failed run also has a nil #value"
    refute_predicate success, :failed?,
                     "B-61: a nil-value success must report #failed? false, staying distinct from a failure"
    assert_predicate failure, :failed?,
                     "B-61: a failed run must report #failed? true though its #value matches the success"
  end

  # B-61: #failed? is true on the trap path too, where #value is nil for the
  # same reason a nil-value success is — a timed-out run must stay tellable
  # from a run that simply returned nil.
  def test_failed_is_true_on_the_trap_path
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    execution = assert_raises(Kobako::TimeoutError) { sandbox.eval("loop { }") }.execution

    assert_predicate execution, :failed?,
                     "B-61: a trapped run's carried Execution must report #failed? true"
  end

  # B-61: a pre-flight failure that enters no invocation — an input rejected
  # before the guest runs — raises with #execution left nil, since no run
  # produced observables to carry.
  def test_a_preflight_failure_carries_no_execution
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    error = assert_raises(Kobako::SandboxError) { sandbox.eval(123) }

    assert_nil error.execution,
               "B-61: a non-String code is rejected before any invocation, so #execution is nil"
  end
end
