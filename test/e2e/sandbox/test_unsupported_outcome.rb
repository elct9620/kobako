# frozen_string_literal: true

require "test_helper"

# E2E — a guest that answers its invocation as unsupported answered rather
# than broke, so the invocation fails as the Sandbox's rather than a trap.
# The real Guest Binary serves every invocation, so the fixture stands in
# for a Guest Binary built without one.
class TestUnsupportedOutcome < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_unsupported_guest.wat")

  def setup
    require_fixture!(FIXTURE_PATH)
  end

  # @behavior OC-052
  def test_an_invocation_the_guest_does_not_serve_is_the_sandboxs_failure
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }

    refute_kind_of Kobako::TrapError, err,
                   "an invocation answered as unsupported through Sandbox#eval must fail as the " \
                   "Sandbox's failure rather than a trap"
  end
end
