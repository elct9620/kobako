# frozen_string_literal: true

require "test_helper"

# E2E — the threads guests run on, kept for the next run and ended once idle
# past their limit. Linux names each thread in /proc, so the child counts the
# guest threads there; the count belongs to the process, so it runs alone.
class TestE2EIdleGuestThreads < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  CHILD = <<~RUBY
    require "kobako"
    def guest_threads = Dir.glob("/proc/self/task/*/comm").count { |comm| File.read(comm).strip == "kobako-guest" }
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release)
    sandbox.bind("Host::Wait", ->(seconds) { sleep(seconds) })
    Array.new(3) { Thread.new { sandbox.eval("Host::Wait.call(0.3)") } }.each(&:join)
    kept = guest_threads
    sleep 11
    ended = guest_threads
    sandbox.eval("1")
    print [kept, ended, guest_threads].inspect
  RUBY

  # @behavior S-232
  def test_a_guests_thread_left_idle_ends
    skip "only Linux names each thread in /proc" unless File.directory?("/proc/self/task")

    assert_equal "[3, 0, 1]", run_ruby(CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: 40),
                 "three Sandbox#eval runs at once must leave three guest threads, which must end after ten " \
                 "seconds idle, and a later run must start one again"
  end
end
