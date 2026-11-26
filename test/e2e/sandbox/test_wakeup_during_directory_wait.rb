# frozen_string_literal: true

require "test_helper"

# E2E — a run waiting on a directory another sandbox's run holds, its thread
# woken by Thread#wakeup, through the real guest binary. The wakeup raises
# nothing, so the wait goes on until the holder lets go.
class TestE2EWakeupDuringDirectoryWait < Minitest::Test
  include E2eGuestHelper
  include Handshake

  def setup
    @tmpfs = Kobako::Filesystem::Tmpfs.new
    @release = Thread::Queue.new
  end

  %i[hold release].each do |gvl|
    # @behavior S-222
    define_method(:"test_a_directory_wait_goes_on_through_a_wakeup_under_#{gvl}") do
      holder = hold_root
      writer = write_once_waiting(gvl)
      writer.wakeup
      @release << true

      assert_equal 1, writer.value.tap { holder.join },
                   "a guest's File.write to a directory another run holds, with gvl: :#{gvl}, its Thread " \
                   "taking Thread#wakeup, must go on waiting and write once the other lets go"
    end
  end

  private

  # A thread whose run holds the root from its mount's block until the
  # release is filled, started once the block holds it.
  def hold_root
    asking = Thread::Queue.new
    holder = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release)
    holder.mount("/data", @tmpfs, access: :read_write) do |_path, access|
      (asking << true) && @release.pop if access == :read_write
      true
    end
    Thread.new { holder.eval('File.write("/data/a", "1")') }.tap { asking.pop }
  end

  # A thread whose run writes into the root under +gvl+, started once it
  # waits on the directory the holder holds.
  def write_once_waiting(gvl)
    waiting = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl, timeout: 30)
                             .mount("/data", @tmpfs, access: :read_write)
    writer = Thread.new { waiting.eval('File.write("/data/b", "2")').value }
    wait_until("the write never waited on the held directory, so its run failed or never asked") do
      waiting.stat(:runs_waiting) == 1 || !writer.alive?
    end
    writer
  end
end
