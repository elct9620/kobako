# frozen_string_literal: true

require "test_helper"

# E2E — a Thread#raise or Thread#kill into a Thread whose run computes. Under
# gvl: :release the guest's engine takes the run's pending signal as it ticks;
# under gvl: :hold the run's caller takes what Ruby delivered each quantum it
# waits for its guest. Either way the run stops within a tick, not once the
# guest would have ended.
class TestE2ERaiseDuringCompute < Minitest::Test
  include E2eGuestHelper

  COMPUTE = "i = 0; i += 1 while i < 200_000_000; i"

  %i[hold release].each do |gvl|
    # @behavior S-230
    define_method(:"test_a_raise_into_a_thread_whose_run_computes_under_#{gvl}_stops_the_guest_as_it_computes") do
      runner = computing(gvl) { |run| raised_within(&run) }
      runner.raise(RuntimeError, "stop")

      raised, seconds = runner.value
      assert_equal ["stop", true], [raised, seconds < 2],
                   "a Thread#raise into a Thread running Sandbox#eval with gvl: :#{gvl}, whose guest computes " \
                   "for seconds, must raise there within moments, took #{seconds.round(2)}s"
    end

    # @behavior S-242
    define_method(:"test_a_kill_of_a_thread_whose_run_computes_under_#{gvl}_ends_that_thread_as_it_computes") do
      started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      computing(gvl, &:call).kill.join
      seconds = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started

      assert_operator seconds, :<, 2,
                      "a Thread#kill of a Thread running Sandbox#eval with gvl: :#{gvl}, whose guest computes " \
                      "for seconds, must end that Thread within moments, took #{seconds.round(2)}s"
    end
  end

  private

  # A Thread running a guest that computes for seconds, handed to +body+ as a
  # callable, once the guest has had time to start.
  def computing(gvl, &body)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl, timeout: 30)
    runner = Thread.new { body.call(-> { sandbox.eval(COMPUTE) }) }
    runner.report_on_exception = false
    sleep 0.2
    runner
  end

  # What the block raised, beside how long it ran.
  def raised_within
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    yield
    [nil, Process.clock_gettime(Process::CLOCK_MONOTONIC) - started]
  rescue RuntimeError => e
    [e.message, Process.clock_gettime(Process::CLOCK_MONOTONIC) - started]
  end
end
