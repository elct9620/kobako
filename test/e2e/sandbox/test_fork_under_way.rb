# frozen_string_literal: true

require "test_helper"

# E2E — a run that starts while a fork is under way, through the real guest
# binary. A `_fork` hook prepended before the first sandbox runs inside
# kobako's own, so the fork stops there with runs held back.
class TestE2EForkUnderWay < Minitest::Test
  include ForkingChild

  RUN_DURING_A_FORK = <<~RUBY
    waiting = sandbox
    forking = pause_a_fork
    run = Thread.new { waiting.eval("1").value }
    Thread.pass until waiting.stat(:runs_waiting) == 1
    beside = fork_within(2)
    interrupted = Thread.new do
      waiting.eval("2").value
    rescue ArgumentError => e
      e.message
    end
    Thread.pass until waiting.stat(:runs_waiting) == 2
    interrupted.raise(ArgumentError, "stop")
    held = run.alive?
    $resume << 1
    print [held, beside, interrupted.value, forking.value, run.value].inspect
  RUBY

  # @behavior S-249
  def test_a_run_starting_while_a_fork_is_under_way_waits_for_it
    assert_equal '[true, :forked, "stop", :forked, 1]', run_forking_child(PAUSING_HOOK + RUN_DURING_A_FORK),
                 "a Sandbox#eval another Thread starts while Kernel#fork stops in a _fork hook must wait until " \
                 "the fork returns and then answer, Thread#raise must end that wait, and a fork beside the " \
                 "waiting run must go on"
  end

  # A hook prepended after the first sandbox stands outside kobako's, so it
  # forks again the moment the gate opens, before the waiting run's thread
  # could take a turn of its own.
  FORK_AGAIN_AS_ONE_RETURNS = <<~RUBY
    queue = Queue.new
    blocked = sandbox.bind("Blocker", Blocker.new(queue))
    $again = false
    Process.singleton_class.prepend(Module.new do
      def _fork
        pid = super
        if $again && pid != 0
          $again = false
          $again_outcome = fork_outcome
        end
        pid
      end
    end)
    print(Array.new(20) do
      forking = pause_a_fork
      run = Thread.new { blocked.eval("Blocker.wait; :ended").value }
      Thread.pass until blocked.stat(:runs_waiting) == 1
      $again = true
      $resume << 1
      forking.value
      queue << 1
      [Array($again_outcome).first, run.value]
    end.uniq.inspect)
  RUBY

  # @behavior S-250
  def test_a_run_waiting_on_a_fork_enters_as_that_fork_returns
    assert_equal "[[:raised, :ended]]", run_forking_child(PAUSING_HOOK + FORK_AGAIN_AS_ONE_RETURNS),
                 "Kernel#fork made at once after a fork returned, beside a Sandbox#eval that waited on that " \
                 "fork and then waits in a Service, must raise RuntimeError, and the run must answer"
  end

  HOOK_RAISES = <<~RUBY
    $fail = false
    Process.singleton_class.prepend(Module.new do
      def _fork
        raise IOError, "hook" if $fail
        super
      end
    end)
    ran = sandbox
    $fail = true
    failed = begin
      fork { exit!(0) }
    rescue IOError => e
      e.class
    end
    $fail = false
    print [failed, ran.eval("1").value, fork_outcome].inspect
  RUBY

  # @behavior S-251
  def test_a_fork_whose_hook_raises_still_lets_runs_in
    assert_equal "[IOError, 1, :forked]", run_forking_child(HOOK_RAISES),
                 "after Kernel#fork raised through a _fork hook prepended before the first Sandbox, a " \
                 "Sandbox#eval must answer and a later fork must go on"
  end
end
