# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox waiting on a directory another Thread's Sandbox holds stops
# when its own run is to stop, so a holder that never lets go cannot keep it.
# The run is a child process the test kills past a deadline, as a wait that
# never stops leaves no Thread of the process able to run a timeout.
class TestE2EDirectoryLockStops < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  DEADLINE = 30

  # One Sandbox holding a directory while its mount's block waits to be told to
  # go on, and one waiting on that directory that either reaches its own timeout
  # or a Timeout interrupts; the holder still running shows neither waited for it.
  CHILD = <<~RUBY
    require "kobako"
    require "timeout"
    asking = Queue.new
    done = Queue.new
    tmpfs = Kobako::Filesystem::Tmpfs.new
    holder = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release)
    holder.mount("/data", tmpfs, access: :read_write) do |_path, access|
      if access == :read_write
        asking << true
        done.pop(timeout: 5)
      end
      true
    end
    first = Thread.new { holder.eval('File.write("/data/a", "1"); true').value }
    asking.pop
    ended = lambda do |sandbox, &wrap|
      wrap.call { sandbox.eval('File.write("/data/b", "2"); :not_stopped').value }
    rescue Kobako::TimeoutError, Timeout::Error => e
      e.class.name
    end
    timed = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), timeout: 0.2).mount("/data", tmpfs, access: :read_write)
    by_timeout = ended.call(timed) { |&run| run.call }
    plain = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM")).mount("/data", tmpfs, access: :read_write)
    by_interrupt = Thread.new { ended.call(plain) { |&run| Timeout.timeout(0.2, &run) } }.value
    still_held = first.alive?
    done << true
    print [by_timeout, by_interrupt, still_held, first.value].inspect
  RUBY

  # @behavior MT-031 MT-032
  def test_a_sandbox_waiting_on_a_directory_stops_at_its_timeout_or_an_interrupt
    assert_equal '["Kobako::TimeoutError", "Timeout::Error", true, true]',
                 run_ruby(CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE),
                 "a Sandbox#eval waiting on a directory another Thread's Sandbox#mount block holds must raise " \
                 "its own timeout, and a Timeout around one must raise, while the block still holds it"
  end
end
