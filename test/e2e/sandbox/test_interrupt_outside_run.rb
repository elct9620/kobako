# frozen_string_literal: true

require "test_helper"

# E2E — an interrupt pending as a wait no Sandbox run brackets begins, through
# the real Guest Binary.
#
# Building an Overlay over a Tmpfs waits for the Tmpfs's root while another
# Thread's run writes there. The building Thread raises on itself under a mask
# that defers it, then lifts the mask just around the build, so the interrupt
# is pending as the wait begins. It belongs to the build, so the Thread's next
# run must not meet it. The child prints what the build raised and what the
# next run answered.
class TestE2EInterruptOutsideRun < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  DEADLINE = 30

  CHILD = <<~RUBY
    require "kobako"
    class Stop < Exception; end
    asking = Queue.new
    done = Queue.new
    tmpfs = Kobako::Filesystem::Tmpfs.new
    lower = Kobako::Filesystem::Tmpfs.new
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
    built = begin
      Thread.handle_interrupt(Stop => :never) do
        Thread.current.raise(Stop)
        Thread.handle_interrupt(Stop => :immediate) do
          Kobako::Filesystem::Overlay.new(lower: [lower], upper: tmpfs)
        end
      end
      :built
    rescue Stop, ArgumentError => e
      e.class.name
    end
    done << true
    first.value
    following = begin
      Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM")).eval("1 + 1").value
    rescue Exception => e
      e.class.name
    end
    print [built, following].inspect
  RUBY

  # @behavior S-217
  def test_an_interrupt_pending_as_an_overlay_build_waits_is_raised_there_and_spares_the_next_run
    assert_equal '["Stop", 2]', run_ruby(CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE),
                 "an interrupt pending as Kobako::Filesystem::Overlay.new starts waiting on a Tmpfs another " \
                 "Thread's Sandbox#eval writes must be raised by Overlay.new, and the Thread's next " \
                 "Sandbox#eval must answer 2"
  end
end
