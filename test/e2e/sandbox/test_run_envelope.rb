# frozen_string_literal: true

require "test_helper"

# The #run invocation-envelope reservation: a guest whose `cabi_realloc`
# traps for want of room ends the invocation as a trap, before the guest
# entry point is reached. Driven by a frozen wat fixture whose allocator
# always traps.
class TestSandboxRunEnvelope < Minitest::Test
  include GuestGuard

  ALLOC_TRAP_FIXTURE_PATH = TestPaths.fixture("minimal_alloc_trap.wat")

  def setup
    require_fixture(ALLOC_TRAP_FIXTURE_PATH)
  end

  # @behavior S-098
  def test_run_raises_trap_error_when_guest_allocator_traps
    sandbox = Kobako::Sandbox.new(wasm_path: ALLOC_TRAP_FIXTURE_PATH)

    err = assert_raises(Kobako::TrapError) { sandbox.run(:Worker) }
    assert_match(/could not allocate input buffer/, err.message,
                 "a guest allocator that traps while #run reserves the invocation envelope " \
                 "must surface as a TrapError saying the input buffer could not be allocated")
  end
end
