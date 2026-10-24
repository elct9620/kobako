# frozen_string_literal: true

require "test_helper"

# E2E — the deadline covers setting the guest up, not only its export call.
# The real Guest Binary sets up too quickly to reach a deadline, so the
# fixture's allocator spins while the host writes its request.
class TestE2ESetupDeadline < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_spinning_alloc.wat")
  DEADLINE = 0.3

  def setup
    require_fixture!(FIXTURE_PATH)
  end

  # @behavior RT-111
  def test_a_guest_that_never_finishes_being_set_up_ends_at_the_deadline
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: DEADLINE)

    assert_raises(Kobako::TimeoutError,
                  "a guest whose allocator never returns through Sandbox#eval must end as the deadline's own " \
                  "trap") { sandbox.eval("1") }
  end
end
