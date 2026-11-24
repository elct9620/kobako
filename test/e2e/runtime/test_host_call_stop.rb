# frozen_string_literal: true

require "test_helper"

# E2E — a host call that returns as its run's timeout passes ends the run
# there, so the guest never reads the EINTR the call answered. Moves waiting
# on each other's directories return at their deadline as often as not just
# before the engine's next tick, where a guest would otherwise read it.
class TestE2EHostCallStop < Minitest::Test
  include E2eGuestHelper

  SEEDED = (0...4).to_h { |i| ["d/f#{i}", "a"] }.merge("e/keep" => "k").freeze

  # A guest moving its own file from +d+ to +e+ and back, over and over.
  MOVES = 'loop { File.rename("/s/d/f%<i>d", "/s/e/f%<i>d"); File.rename("/s/e/f%<i>d", "/s/d/f%<i>d") }'

  # @behavior RT-426
  def test_a_move_its_timeout_ends_ends_the_run
    raised = Array.new(3) { moves_beside_each_other }.flatten.tally

    assert_equal({ "Kobako::TimeoutError" => 12 }, raised,
                 "runs moving files between two directories of one Tmpfs must each raise Kobako::TimeoutError " \
                 "as their timeout passes, never the Errno::EINTR a waiting move answered")
  end

  private

  def moves_beside_each_other
    tmpfs = Kobako::Filesystem::Tmpfs.new(SEEDED)
    Array.new(4) do |i|
      Thread.new do
        Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.3).mount("/s", tmpfs, access: :read_write)
                       .eval(format(MOVES, i: i))
      rescue StandardError => e
        e.class.name
      end
    end.map(&:value)
  end
end
