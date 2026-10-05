# frozen_string_literal: true

require "test_helper"

# Unit tests for where an argument failure comes from. The binding mismatch
# is pinned in test_dispatcher.rb; a Service validating its own arguments
# reports the same way, so the guest rescues one class for both.
class TestDispatchArgumentFault < Minitest::Test
  include DispatcherHelpers

  # @behavior T-230
  def test_argument_error_raised_inside_the_service_body_is_an_argument_failure
    @registry.bind("Service::Strict", ->(x) { raise ArgumentError, "bad #{x}" })

    answer = reify(dispatch(build_call("Service::Strict", "call", [1], {})))

    assert_equal [false, "argument"], [answer.ok?, answer.payload.type],
                 "an ArgumentError a Service raises in its own body through dispatch must be an argument failure"
  end
end
