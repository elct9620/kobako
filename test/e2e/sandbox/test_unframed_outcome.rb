# frozen_string_literal: true

require "test_helper"

# E2E — a guest that returns without an Outcome the envelope can frame
# leaves no record to attribute, so the invocation settles as a trap.
# The real Guest Binary always writes one, so the fixture stands in for a
# guest runtime past reasoning about.
class TestUnframedOutcome < Minitest::Test
  include GuestGuard

  UNFRAMED = TestPaths.fixture("minimal_unframed_outcome.wat")

  def setup
    require_fixture!(UNFRAMED)
  end

  # @behavior OC-001
  # The message stays in caller vocabulary — a zero length is a wire
  # detail a Host App cannot act on, so it never appears in it.
  def test_a_guest_that_wrote_nothing_settles_as_a_trap
    sandbox = Kobako::Sandbox.new(wasm_path: UNFRAMED)

    err = assert_raises(Kobako::TrapError) { sandbox.eval("nil") }

    assert_equal "Sandbox#eval failed: Sandbox exited without producing a result", err.message,
                 "a guest writing no Outcome through Sandbox#eval must trap saying it produced none"
  end

  # @behavior OC-002
  # The next invocation runs on a fresh instance, so the message names the
  # failure and asks nothing of the Sandbox the caller holds.
  def test_a_result_the_envelope_cannot_frame_settles_as_a_trap
    sandbox = Kobako::Sandbox.new(wasm_path: UNFRAMED)
    sandbox.preload(code: "Worker = 1", name: :Worker)

    err = assert_raises(Kobako::TrapError) { sandbox.run(:Worker) }

    assert_equal "Sandbox#run failed: Sandbox produced an unrecognised result", err.message,
                 "an unframeable Outcome through Sandbox#run must trap without asking the caller " \
                 "to discard the Sandbox"
  end
end
