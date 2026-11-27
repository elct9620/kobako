# frozen_string_literal: true

require "test_helper"

# E2E — a Thread#raise or Thread#kill into a thread whose run computes. Under
# gvl: :release the guest's engine takes the run's pending signal as it ticks;
# under gvl: :hold the run's caller takes what Ruby delivered each quantum it
# waits for its guest. Either way the run stops as it computes. The guest
# never ends on its own, so its deadline is reached only when the raise or the
# kill never landed, which turns the test red.
class TestE2ERaiseDuringCompute < Minitest::Test
  include E2eGuestHelper

  COMPUTE = "Started.call; loop { }"

  # Far past any slowdown, so a run reaching it means the raise or kill never
  # landed.
  MAX_RUN_TIME = 30

  %i[hold release].each do |gvl|
    # @behavior S-230
    define_method(:"test_a_raise_into_a_thread_whose_run_computes_under_#{gvl}_stops_the_guest_as_it_computes") do
      assert_equal "stop", raise_into_computing(gvl),
                   "a Thread#raise into a Thread running Sandbox#eval with gvl: :#{gvl}, whose guest computes " \
                   "without end, must raise there as it computes rather than at the run's deadline"
    end

    # @behavior S-242
    define_method(:"test_a_kill_of_a_thread_whose_run_computes_under_#{gvl}_ends_that_thread_as_it_computes") do
      assert_empty kill_computing(gvl),
                   "a Thread#kill of a Thread running Sandbox#eval with gvl: :#{gvl}, whose guest computes " \
                   "without end, must end that Thread as it computes rather than at the run's deadline"
    end
  end

  private

  # What the run raises once another thread raises "stop" into it, or
  # +:deadline+ if only its deadline ended it.
  def raise_into_computing(gvl)
    runner = start_computing(gvl) do |run|
      run.call
    rescue RuntimeError => e
      e.message
    rescue Kobako::TimeoutError
      :deadline
    end
    runner.raise(RuntimeError, "stop")
    runner.value
  end

  # How the run ended on its own once its thread was killed: nothing, when the
  # kill ended it.
  def kill_computing(gvl)
    ended = []
    start_computing(gvl) do |run|
      run.call
      ended << :returned
    rescue Kobako::TimeoutError
      ended << :deadline
    end.kill.join
    ended
  end

  # A thread running a guest that computes without end, handed to +body+ as
  # a callable, once the guest has called back to say it started.
  def start_computing(gvl, &body)
    started = Thread::Queue.new
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl, timeout: MAX_RUN_TIME)
                             .bind("Started", -> { started << true })
    runner = Thread.new { body.call(-> { sandbox.eval(COMPUTE) }) }
    runner.report_on_exception = false
    started.pop
    runner
  end
end
