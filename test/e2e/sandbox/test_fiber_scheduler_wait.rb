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
    abort "no-worker-pool" unless defined?(IO::Event::WorkerPool)
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release, timeout: 30)
  RUBY

  # Each guest marks its file on a shared Tmpfs, then computes until the
  # other's is there, so both finish only if the scheduler runs the other
  # run's Fiber while the first waits on its guest; run one after the other,
  # they would reach their deadline instead. A guest spinning on a check
  # allocates as fast as it asks, so its memory is not capped.
  OVERLAP_CHILD = PRELUDE + <<~RUBY
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release, timeout: 30, memory_limit: nil)
                             .mount("/t", Kobako::Filesystem::Tmpfs.new, access: :read_write)
    meet = ->(me, other) { %(File.write("/t/\#{me}", "1"); nil until File.exist?("/t/\#{other}"); :met) }
    met = Sync do |task|
      [%w[a b], %w[b a]].map { |me, other| task.async { sandbox.eval(meet.(me, other)).value } }.map(&:wait)
    end
    print met.inspect
  RUBY

  # The guest says it started, then computes without end, so only the stop
  # ends the run before its deadline.
  STOP_CHILD = PRELUDE + <<~RUBY
    started = Thread::Queue.new
    sandbox.bind("Started", -> { started << true })
    ended = Sync do |task|
      run = task.async do
        sandbox.eval("Started.call; loop { }")
      rescue Kobako::TimeoutError
        :deadline
      end
      started.pop
      run.stop
      run.wait
    end
    print [ended == :deadline, sandbox.eval("1 + 1").value].inspect
  RUBY

  # @behavior S-234
  def test_a_run_waiting_on_its_guest_lets_a_fiber_scheduler_run_other_fibers
    assert_equal "[:met, :met]", run_with_worker_pool(OVERLAP_CHILD),
                 "two Sandbox#eval runs with gvl: :release on Fibers of one Thread under Async's worker pool, " \
                 "each guest only computing, must overlap while a ticking Fiber goes on"
  end

  # @behavior S-235
  def test_a_fiber_scheduler_stopping_a_fiber_whose_run_waits_ends_that_run
    assert_equal "[false, 2]", run_with_worker_pool(STOP_CHILD),
                 "an Async task stopped while its Sandbox#eval with gvl: :release waits on a guest computing " \
                 "without end must end before the run's deadline, and the next run must answer"
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
