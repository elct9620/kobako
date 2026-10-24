# frozen_string_literal: true

require "test_helper"

# E2E — the deadline covers setting the guest up, not only its export call.
# The real Guest Binary sets up too quickly to reach a deadline, so the
# fixture's allocator spins while the host writes its request.
class TestE2ESetupDeadline < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_spinning_alloc.wat")
  STALLED_START_PATH = TestPaths.fixture("minimal_spinning_start.wat")
  DEADLINE = 0.3

  def setup
    require_fixture!(FIXTURE_PATH)
    require_fixture!(STALLED_START_PATH)
  end

  # The construction-time ABI check instantiates the artifact once, so a
  # start function that never returns stalls there, before any invocation.
  # @behavior RT-112
  def test_a_guest_binary_that_never_finishes_instantiating_fails_construction
    error = assert_raises(Kobako::SetupError,
                          "a Guest Binary whose start never returns through Sandbox.new must fail " \
                          "construction once the setup deadline passes") do
      Kobako::Sandbox.new(wasm_path: STALLED_START_PATH, setup_timeout: DEADLINE)
    end
    assert_match(/within/, error.message,
                 "the construction failure must say the setup deadline passed, not that the artifact is broken")
  end

  # @behavior RT-111
  def test_a_guest_that_never_finishes_being_set_up_ends_at_the_deadline
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: DEADLINE)

    assert_raises(Kobako::TimeoutError,
                  "a guest whose allocator never returns through Sandbox#eval must end as the deadline's own " \
                  "trap") { sandbox.eval("1") }
  end
end
