# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — a Service answer the host cannot write. The Service returns
# whatever host object it holds and the boundary converts it, so a value that
# nests without bound fails while the dispatch is still being answered. The
# outbound yield half — what the Service sends into the block — lives in
# test_yield_value_refusal.rb.
#
# Each case runs on a stack of its own: refusing the value costs the thread
# that takes it (see StackQuarantine).
class TestE2EAnswerValueRefusal < Minitest::Test
  include E2eGuestHelper
  include StackQuarantine

  CALL_ONCE = "Probe::Answer.call"

  # The Service ran and produced something; only it can change what that is.
  # A trap would tell the Host App to discard the Sandbox for a failure that
  # is neither the guest's nor the runtime's.
  def test_an_unwritable_answer_reaches_the_host_app_as_a_service_failure
    error = in_a_spendable_stack do
      assert_raises(Kobako::ServiceError) { cyclic_sandbox.eval(CALL_ONCE) }
    end

    assert_instance_of Kobako::ServiceError, error,
                       "a Service answer the host cannot write through #eval must reach the Host " \
                       "App as a Service failure, not as an exchange that produced no Service " \
                       "outcome and not as a trap"
  end

  def test_an_unwritable_answer_answers_in_kobakos_own_wording
    error = in_a_spendable_stack do
      assert_raises(Kobako::ServiceError) { cyclic_sandbox.eval(CALL_ONCE) }
    end

    refute_match(/Kobako::/, error.message,
                 "the refusal is kobako's own, so it must not wear the <class>: <message> " \
                 "shape a Service exception crosses in")
  end

  # The guest may rescue it like any other Service failure, which is what
  # separates it from a trap: the invocation goes on to produce a value.
  RESCUING = <<~RUBY
    begin
      Probe::Answer.call
      :unreached
    rescue
      :rescued
    end
  RUBY

  def test_the_guest_may_rescue_an_unwritable_answer_and_carry_on
    seen = in_a_spendable_stack { cyclic_sandbox.eval(RESCUING).value }

    assert_equal :rescued, seen,
                 "a guest rescuing a Service answer the host could not write must go on to " \
                 "finish the invocation, so the failure is the dispatch's rather than the run's"
  end

  private

  # A Service returning a self-referential Array — representable in type, but
  # nesting without bound, so the host cannot write it.
  def cyclic_sandbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Answer", -> { [].tap { |a| a << a } })
    sandbox
  end
end
