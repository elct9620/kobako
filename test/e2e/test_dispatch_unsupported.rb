# frozen_string_literal: true

require "test_helper"

# The real Guest Binary only ever sends a call, so a hand-written guest
# sends the host an upcall under an unassigned tag and reports which arm
# came back as its result.
class TestDispatchUnsupported < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_unknown_upcall.wat")

  def setup
    require_fixture!(FIXTURE_PATH)
  end

  # @behavior T-277
  def test_a_message_the_host_does_not_serve_is_answered_as_unsupported
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal true, sandbox.eval("").value,
                 "an upcall under a tag the host does not serve through Sandbox#eval must be " \
                 "answered on the unsupported arm rather than failing the exchange"
  end
end
