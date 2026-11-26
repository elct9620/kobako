# frozen_string_literal: true

require "test_helper"

# E2E — a run held back by a fork under way, as a Sandbox counts its runs
# waiting, through the real guest binary.
class TestE2EForkWaitingCount < Minitest::Test
  include ForkingChild

  COUNTED_WHILE_WAITING = <<~RUBY
    queue = Queue.new
    blocker = Blocker.new(queue)
    blocked = sandbox.bind("Blocker", blocker)
    forking = pause_a_fork
    run = Thread.new { blocked.eval("Blocker.wait; :ended").value }
    Thread.pass until blocked.stat(:runs_waiting) == 1
    $resume << 1
    forking.value
    blocker.await_arrival
    entered = blocked.stat(:runs_waiting)
    queue << 1
    print [entered, run.value].inspect
  RUBY

  # @behavior S-265
  def test_a_run_waiting_on_a_fork_counts_among_the_runs_waiting
    assert_equal "[0, :ended]", run_forking_child(PAUSING_HOOK + COUNTED_WHILE_WAITING),
                 "Sandbox#stat(:runs_waiting) must count a Sandbox#eval waiting while Kernel#fork stops in a " \
                 "_fork hook, and count no run once the fork returned and the run went on into a Service"
  end
end
