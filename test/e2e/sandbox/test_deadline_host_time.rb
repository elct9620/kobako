# frozen_string_literal: true

require "test_helper"

# E2E — what the deadline does while host code runs, through real mruby.
# The deadline measures the whole invocation, host time included, but it
# can only stop guest code: a Service running past it finishes, and the
# run ends once control is back in the guest.
class TestE2EDeadlineHostTime < Minitest::Test
  include E2eGuestHelper

  # @behavior S-169
  def test_the_deadline_never_interrupts_a_service
    finished = []
    sandbox = slow_service_sandbox(timeout: 0.05, finished: finished)

    assert_raises(Kobako::TimeoutError) { sandbox.eval("Slow::Wait.call; :after") }

    assert_equal [true], finished, "a Service running past the deadline through #eval must complete"
  end

  # What follows the call takes no measurable time, so the run ending
  # there is the deadline having counted the Service's time.
  # @behavior S-170
  def test_the_deadline_cuts_the_run_once_control_returns_to_the_guest
    sandbox = slow_service_sandbox(timeout: 0.05, finished: [])

    assert_raises(Kobako::TimeoutError,
                  "a Service running past the deadline through #eval must cut the run short " \
                  "once control returns to the guest") do
      sandbox.eval("Slow::Wait.call; :after")
    end
  end

  # @behavior S-179
  def test_a_services_time_counts_in_the_reported_wall_time
    finished = []

    usage = slow_service_sandbox(timeout: 5, finished: finished).eval("Slow::Wait.call").usage

    assert_operator usage.wall_time, :>=, 0.2,
                    "the wall time a run reports must include its Service callback's time"
  end

  private

  # Sleeps well past the deadline, then records that it finished.
  def slow_service_sandbox(timeout:, finished:)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: timeout)
    sandbox.bind("Slow::Wait", lambda do
      sleep 0.2
      finished << true
      :done
    end)
    sandbox
  end
end
