# frozen_string_literal: true

require "test_helper"

# E2E — a run releasing the GVL waiting on its guest under a Fiber scheduler
# that runs blocking operations on another thread. Async does so with its
# worker pool, which io-event builds only where Ruby offers the scheduler
# hook's operations, and which a process turns on as it starts, so each case
# runs in a child.
class TestE2EFiberSchedulerWait < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  PRELUDE = <<~RUBY
    require "kobako"
    require "async"
    def clock = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    def timed = (started = clock; yield; clock - started)
    abort "no-worker-pool" unless defined?(IO::Event::WorkerPool)
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release, timeout: 30)
  RUBY

  OVERLAP_CHILD = PRELUDE + <<~RUBY
    compute = "i = 0; i += 1 while i < 5_000_000; i"
    one = timed { sandbox.eval(compute) }
    ticks = 0
    two = timed do
      Sync do |task|
        ticker = task.async { loop { sleep 0.01; ticks += 1 } }
        Array.new(2) { task.async { sandbox.eval(compute) } }.each(&:wait)
        ticker.stop
      end
    end
    print [two < one * 1.6, ticks >= one / 0.02].inspect
  RUBY

  STOP_CHILD = PRELUDE + <<~RUBY
    stopped = timed do
      Sync do |task|
        run = task.async { sandbox.eval("i = 0; i += 1 while i < 2_000_000_000; i") }
        sleep 0.2
        run.stop
        run.wait
      end
    end
    print [stopped < 2, sandbox.eval("1 + 1").value].inspect
  RUBY

  # @behavior S-234
  def test_a_run_waiting_on_its_guest_lets_a_fiber_scheduler_run_other_fibers
    assert_equal "[true, true]", run_with_worker_pool(OVERLAP_CHILD),
                 "two Sandbox#eval runs with gvl: :release on Fibers of one Thread under Async's worker pool, " \
                 "each guest only computing, must overlap while a ticking Fiber goes on"
  end

  # @behavior S-235
  def test_a_fiber_scheduler_stopping_a_fiber_whose_run_waits_ends_that_run
    assert_equal "[true, 2]", run_with_worker_pool(STOP_CHILD),
                 "an Async task stopped while its Sandbox#eval with gvl: :release waits on a computing guest " \
                 "must end within moments, and the next run must answer"
  end

  private

  def run_with_worker_pool(script)
    env = { "KOBAKO_WASM" => REAL_WASM, "ASYNC_SCHEDULER_WORKER_POOL" => "true" }
    run_ruby(script, env: env, deadline: 60)
  rescue Minitest::Assertion => e
    raise unless e.message.include?("no-worker-pool")

    skip "Async's worker pool needs a Ruby whose scheduler hook it can build on"
  end
end
