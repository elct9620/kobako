# frozen_string_literal: true

require "test_helper"

# E2E — a fork that waits for the runs on other Threads, through the real
# Guest Binary. Each run is held up by a Service that blocks on a Queue the
# script fills once the fork has started waiting.
class TestE2EForkWait < Minitest::Test
  include ForkingChild

  WAITS_FOR_RUN = <<~RUBY
    queue = Queue.new
    blocked = sandbox.bind("Blocker", Blocker.new(queue))
    run = Thread.new { blocked.eval("Blocker.wait; :ended").value }
    Thread.pass until run.status == "sleep"
    $stderr = StringIO.new
    Thread.new { sleep 0.3; queue << 1 }
    answered = child_answers?(sandbox)
    warnings = $stderr.string.lines.size
    $stderr = STDERR
    print [run.status, answered, warnings].inspect
  RUBY

  # @behavior S-236
  def test_a_fork_waits_for_another_threads_run_to_end
    assert_equal "[false, true, 1]", run_forking_child(WAITS_FOR_RUN),
                 "Kernel#fork while another Thread's Sandbox#eval waits in a Service must warn once, return " \
                 "only after that run ended, and leave a child whose Sandbox answers"
  end

  RUN_STARTS_WHILE_WAITING = <<~RUBY
    inner = sandbox
    go = Queue.new
    joiner = Object.new
    joiner.define_singleton_method(:join) { go.pop; Thread.new { inner.eval("2").value }.value }
    outer = sandbox.bind("Joiner", joiner)
    run = Thread.new { outer.eval("Joiner.join").value }
    Thread.pass until run.status == "sleep"
    $stderr = StringIO.new
    Thread.new { sleep 0.3; go << 1 }
    answered = child_answers?(sandbox)
    $stderr = STDERR
    print [run.value, answered].inspect
  RUBY

  # @behavior S-237
  def test_a_run_that_starts_while_a_fork_waits_still_runs
    assert_equal "[2, true]", run_forking_child(RUN_STARTS_WHILE_WAITING),
                 "a Sandbox#eval another Thread starts while a fork waits for the run that joins it must answer, " \
                 "and the fork must return"
  end

  INTERRUPTED_WAIT = <<~RUBY
    queue = Queue.new
    blocked = sandbox.bind("Blocker", Blocker.new(queue))
    run = Thread.new { blocked.eval("Blocker.wait; :ended").value }
    Thread.pass until run.status == "sleep"
    $stderr = StringIO.new
    forking = Thread.new do
      fork_outcome
    rescue ArgumentError => e
      e.message
    end
    Thread.pass until forking.status == "sleep"
    forking.raise(ArgumentError, "stop")
    outcome = forking.value
    queue << 1
    run.value
    $stderr = STDERR
    print [outcome, child_answers?(sandbox)].inspect
  RUBY

  # @behavior S-239
  def test_an_interrupt_ends_a_forks_wait
    assert_equal '["stop", true]', run_forking_child(INTERRUPTED_WAIT),
                 "Thread#raise into a Thread whose fork waits for another Thread's Sandbox#eval must raise " \
                 "there, and a later fork must go on"
  end
end
