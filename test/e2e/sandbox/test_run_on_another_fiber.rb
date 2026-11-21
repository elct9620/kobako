# frozen_string_literal: true

require "test_helper"

# E2E — a run started on one Fiber while another Fiber on the same Thread is
# suspended inside its own run's Service. Each run's guest frames stand on the
# Thread's one stack of guest calls, so the later run refuses before its guest
# starts, and the suspended run is left to finish.
class TestE2ERunOnAnotherFiber < Minitest::Test
  include E2eGuestHelper

  # @behavior S-225
  def test_a_run_on_another_fiber_refuses_while_a_suspended_run_is_unfinished
    %i[hold release].each do |gvl|
      sandbox = pausing_sandbox(gvl)
      suspended = Fiber.new { sandbox.eval('Host::Pause.call("a")').value }
      suspended.resume
      refusal = Fiber.new { refusal { sandbox.eval("1 + 1") } }.resume

      assert_equal %w[ThreadError a], [refusal, suspended.resume],
                   "Sandbox#eval (gvl: #{gvl}) on a Fiber while another Fiber on the same Thread is suspended " \
                   "in its run's Service must raise ThreadError, and the suspended run must still answer"
    end
  end

  # @behavior S-225
  def test_a_run_nested_on_the_suspended_runs_own_fiber_starts
    inner = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Host::Resumed", lambda {
      Fiber.yield
      inner.eval("1 + 1").value
    })
    suspended = Fiber.new { sandbox.eval("Host::Resumed.call").value }
    suspended.resume

    assert_equal 2, suspended.resume,
                 "a Sandbox#eval a Service runs once its suspended Fiber is resumed must answer as usual"
  end

  private

  def pausing_sandbox(gvl)
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl).tap do |sandbox|
      sandbox.bind("Host::Pause", lambda { |tag|
        Fiber.yield
        tag
      })
    end
  end

  def refusal
    yield
    nil
  rescue ThreadError => e
    e.class.name
  end
end
