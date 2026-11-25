# frozen_string_literal: true

require "test_helper"

# E2E — a fork right after the collector freed a fiber suspended in its run,
# through the real guest binary. The run is in progress until its guest ends,
# so a fork either raises or leaves a child that takes the run's locks.
class TestE2EForkAfterCollection < Minitest::Test
  include ForkingChild

  FORK_AS_A_COLLECTED_RUN_ENDS = <<~RUBY
    require "weakref"
    tmpfs = Kobako::Filesystem::Tmpfs.new
    parking = Kobako::Sandbox.new(wasm_path: WASM).mount("/data", tmpfs, access: :read_write) do |_path, access|
      Fiber.yield(:parked) if access == :read_write
    end
    writer = Kobako::Sandbox.new(wasm_path: WASM).mount("/data", tmpfs, access: :read_write)
    outcomes = Array.new(20) do
      fiber = Thread.new do
        parked = Fiber.new { parking.eval('File.write("/data/a", "1")') }
        parked.resume
        WeakRef.new(parked)
      end.value
      100.times { fiber.weakref_alive? ? GC.start : break }
      abort "100 collections kept a Fiber whose Thread ended; something else reaches it" if fiber.weakref_alive?
      pid = fork do
        Thread.new { sleep 2; exit!(1) }
        writer.eval('File.write("/data/b", "2")')
        exit!(0)
      end
      Process.wait(pid)
      $?.success? ? :wrote : :stuck
    rescue RuntimeError
      :raised
    end
    later = 40.times { (outcome = fork_outcome) == :forked ? (break outcome) : sleep(0.05) }
    print [outcomes.uniq - %i[wrote raised], later].inspect
  RUBY

  # @behavior S-252
  def test_a_fork_while_a_collected_runs_guest_ends_raises
    assert_equal "[[], :forked]", run_forking_child(FORK_AS_A_COLLECTED_RUN_ENDS),
                 "Kernel#fork right after the collector freed a Fiber suspended in its Sandbox#eval's mount " \
                 "block must raise RuntimeError, or leave a child that writes where the run held, and a fork " \
                 "once that guest ended must go on"
  end
end
