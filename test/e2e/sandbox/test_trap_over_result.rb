# frozen_string_literal: true

require "test_helper"

# E2E — a trap outranks whatever result the guest had already written.
# The fixture's Outcome buffer holds a well-formed nil Result while its
# entry points trap, so only reading the trap first settles the
# invocation as what it was.
class TestTrapOverResult < Minitest::Test
  include GuestGuard

  TRAPPING = TestPaths.fixture("minimal_trap_after_result.wat")

  def setup
    require_fixture!(TRAPPING)
  end

  # @behavior OC-050
  def test_a_trap_settles_the_invocation_though_a_result_was_written
    sandbox = Kobako::Sandbox.new(wasm_path: TRAPPING)
    sandbox.preload(code: "Worker = 1", name: :Worker)

    { eval: -> { sandbox.eval("nil") }, run: -> { sandbox.run(:Worker) } }.each do |verb, invoke|
      assert_raises(Kobako::TrapError,
                    "##{verb} over a guest that trapped after writing a result must settle as a trap") do
        invoke.call
      end
    end
  end
end
