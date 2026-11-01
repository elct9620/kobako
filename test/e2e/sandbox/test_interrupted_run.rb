# frozen_string_literal: true

require "test_helper"

# E2E — what a Thread is left with once the Host App interrupts its run,
# through the real Guest Binary.
#
# An interrupt skipping the native frames of a run would leave the Thread
# believing it gave up the GVL, which aborts the process at its next Service
# call, so the run is a child process and a crash fails through its status.
class TestE2EInterruptedRun < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  DEADLINE = 60

  NEXT_RUN_CHILD = <<~RUBY
    require "kobako"
    require "timeout"
    released = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release, timeout: nil)
    begin
      Timeout.timeout(0.05) { released.eval("i = 0; while i < 30_000_000; i += 1; end; i") }
      print "not interrupted "
    rescue Timeout::Error
      print "interrupted "
    end
    held = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"))
    held.bind("Host::Answer", -> { 41 })
    print held.eval("Host::Answer.call + 1").value
  RUBY

  # @behavior S-207
  def test_a_thread_whose_run_was_interrupted_runs_its_next_sandbox_as_usual
    assert_equal "interrupted 42", run_ruby(NEXT_RUN_CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE),
                 "a Timeout around a Sandbox#eval that released the GVL must raise Timeout::Error, and the next " \
                 "Sandbox#eval on that Thread must answer what its Service returned"
  end

  WAITING_CHILD = <<~RUBY
    require "kobako"
    require "timeout"
    asking = Queue.new
    tmpfs = Kobako::Filesystem::Tmpfs.new
    holder = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release)
    holder.mount("/data", tmpfs, access: :read_write) do |_path, access|
      if access == :read_write
        asking << true
        sleep 0.3
      end
      true
    end
    first = Thread.new { holder.eval('File.write("/data/a", "1")') }
    asking.pop
    waiting = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM")).mount("/data", tmpfs, access: :read_write)
    begin
      Timeout.timeout(0.05) { waiting.eval('File.write("/data/b", "2")') }
      print "returned"
    rescue Timeout::Error
      print "Timeout::Error"
    end
    first.join
  RUBY

  # @behavior S-208
  def test_an_interrupt_delivered_while_a_run_waits_reaches_the_host_app_once_the_run_ends
    assert_equal "Timeout::Error", run_ruby(WAITING_CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE),
                 "a Timeout firing while a Sandbox#eval waits on a directory must raise Timeout::Error in the " \
                 "Host App once the run ends"
  end
end
