# frozen_string_literal: true

require "test_helper"

# E2E — two runs keeping the GVL whose guests wait on each other through one
# +Tmpfs+. Each caller hands the GVL over while it waits for its guest, so both
# guests go on at once; served one at a time, the first would wait forever.
class TestE2EGvlTurnsSideBySide < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  SIDE_BY_SIDE = <<~RUBY
    require "kobako"
    tmpfs = Kobako::Filesystem::Tmpfs.new
    turns = ->(me, peer) do
      "[1, 2].each { |n| File.write('/s/\#{me}' + n.to_s, '1'); " \
        "nil until File.exist?('/s/\#{peer}' + n.to_s) }; :\#{me}"
    end
    runs = [%w[a b], %w[b a]].map do |me, peer|
      sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :hold, timeout: nil)
                               .mount("/s", tmpfs, access: :read_write)
      Thread.new { sandbox.eval(turns.(me, peer)).value }
    end
    print runs.map(&:value).inspect
  RUBY

  # @behavior S-246
  def test_guests_of_two_runs_keeping_the_gvl_go_on_side_by_side
    assert_equal "[:a, :b]", run_ruby(SIDE_BY_SIDE),
                 "two Sandbox#eval calls with gvl: :hold on two Threads, whose guests each write a file on one " \
                 "Tmpfs and compute until the other's appears, twice in turn, must both answer"
  end
end
