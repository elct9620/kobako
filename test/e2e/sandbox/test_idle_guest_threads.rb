# frozen_string_literal: true

require "test_helper"

# E2E — the threads guests run on, kept for the next run and ended once idle
# past their limit, after which a run still answers. Linux names each thread
# in /proc, so the child counts the guest threads there; the count belongs to
# the process, so it runs alone. A thread that ends between the listing and
# its read counts as gone.
class TestE2EIdleGuestThreads < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  GUEST_THREADS = <<~RUBY
    require "kobako"
    def guest_threads = Dir.glob("/proc/self/task/*/comm").count { |comm| (File.read(comm) rescue "").strip == "kobako-guest" }
  RUBY

  CHILD = <<~RUBY.freeze
    #{GUEST_THREADS}
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release)
    sandbox.bind("Host::Wait", ->(seconds) { sleep(seconds) })
    Array.new(3) { Thread.new { sandbox.eval("Host::Wait.call(0.3)") } }.each(&:join)
    kept = guest_threads
    # The idle limit is the product's ten seconds; past twenty, they never end.
    give_up = Process.clock_gettime(Process::CLOCK_MONOTONIC) + 20
    sleep 0.05 until guest_threads.zero? || Process.clock_gettime(Process::CLOCK_MONOTONIC) > give_up
    ended = guest_threads
    answer = sandbox.eval("1 + 1").value
    print [kept, ended, answer, guest_threads].inspect
  RUBY

  ONE_AFTER_ANOTHER = <<~RUBY.freeze
    #{GUEST_THREADS}
    counts = %i[hold release].map do |gvl|
      sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: gvl)
      200.times { sandbox.eval("1") }
      guest_threads
    end
    print counts.inspect
  RUBY

  # @behavior S-232
  def test_runs_one_after_another_take_the_thread_the_last_one_left
    skip "only Linux names each thread in /proc" unless File.directory?("/proc/self/task")

    assert_equal "[1, 1]", run_ruby(ONE_AFTER_ANOTHER, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: 40),
                 "200 Sandbox#eval runs one after another, under gvl: :hold and then :release, must leave the " \
                 "process one guest thread, each run taking the thread the run before it left"
  end

  # @behavior S-232
  def test_a_run_after_the_guest_threads_were_left_idle_still_answers
    skip "only Linux names each thread in /proc" unless File.directory?("/proc/self/task")

    assert_equal "[3, 0, 2, 1]", run_ruby(CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: 40),
                 "three Sandbox#eval runs at once must leave three guest threads, which must end after ten " \
                 "seconds idle, and a later Sandbox#eval(\"1 + 1\") must answer 2 on one guest thread it starts"
  end
end
