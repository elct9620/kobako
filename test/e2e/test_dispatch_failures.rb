# frozen_string_literal: true

require "test_helper"

# E2E — the failure classes a guest call site raises, through real mruby.
# Each failed dispatch reaches the guest as an exception it may rescue: an
# exchange the host could not complete as the wire-level failure, and every
# way a Service call can fail under one base class.
class TestE2EDispatchFailures < Minitest::Test
  include E2eGuestHelper

  # @behavior T-244
  def test_an_internal_failure_is_the_wire_level_failure_the_guest_may_rescue
    result = internal_failure_sandbox.eval(<<~RUBY).value
      begin
        Probe::Call.call
      rescue Kobako::Transport::Error
        :rescued
      end
    RUBY

    assert_equal :rescued, result,
                 "a dispatch the host answers as an internal failure through #eval must raise " \
                 "Kobako::Transport::Error at the guest call site, where the guest may rescue it"
  end

  # @behavior T-244
  def test_an_unrescued_internal_failure_fails_the_invocation_as_a_sandbox_failure
    assert_raises(Kobako::SandboxError,
                  "an internal dispatch failure left unrescued through #eval must fail the " \
                  "invocation as a Sandbox failure") do
      internal_failure_sandbox.eval("Probe::Call.call")
    end
  end

  # One raises in its own body, one names a path left unfilled, and one
  # is handed an argument it does not take.
  RESCUED_BY_BASE_CLASS = <<~RUBY
    [-> { Probe::Raises.call }, -> { Probe::Unfilled.call }, -> { Probe::Takes.call }].map do |call|
      call.call
      :not_raised
    rescue Kobako::ServiceError
      :rescued
    end
  RUBY

  # @behavior T-245
  def test_rescuing_the_service_failure_base_class_covers_every_failed_call
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Raises", -> { raise "boom" })
    sandbox.bind("Probe::Unfilled")
    sandbox.bind("Probe::Takes", ->(value) { value })

    assert_equal %i[rescued rescued rescued], sandbox.eval(RESCUED_BY_BASE_CLASS).value,
                 "guest code rescuing Kobako::ServiceError through #eval must also catch a call " \
                 "that reached no Service and one whose arguments did not fit"
  end

  private

  # A Service raising the class the host reserves for running out of
  # references makes the host answer as an internal failure, the one
  # answer a guest cannot provoke through the wire on its own.
  def internal_failure_sandbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Call", -> { raise Kobako::HandleExhaustedError, "out of references" })
    sandbox
  end
end
