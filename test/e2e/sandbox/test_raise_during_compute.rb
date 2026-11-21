# frozen_string_literal: true

require "test_helper"

# E2E — a Thread#raise into a Thread whose run releases the GVL while its
# guest only computes. The guest's engine takes the run's pending signal as it
# ticks, so the raise stops the guest within a tick rather than once the guest
# would have ended.
class TestE2ERaiseDuringCompute < Minitest::Test
  include E2eGuestHelper

  COMPUTE = "i = 0; i += 1 while i < 200_000_000; i"

  # @behavior S-230
  def test_a_raise_into_a_thread_whose_run_releases_the_gvl_stops_the_guest_as_it_computes
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release, timeout: 30)
    runner = Thread.new { raised_within { sandbox.eval(COMPUTE) } }.tap { |t| t.report_on_exception = false }
    sleep 0.2
    runner.raise(RuntimeError, "stop")

    raised, seconds = runner.value
    assert_equal ["stop", true], [raised, seconds < 2],
                 "a Thread#raise into a Thread running Sandbox#eval with gvl: :release, whose guest computes " \
                 "for seconds, must raise there within moments, took #{seconds.round(2)}s"
  end

  private

  # What the block raised, beside how long it ran.
  def raised_within
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    yield
    [nil, Process.clock_gettime(Process::CLOCK_MONOTONIC) - started]
  rescue RuntimeError => e
    [e.message, Process.clock_gettime(Process::CLOCK_MONOTONIC) - started]
  end
end
