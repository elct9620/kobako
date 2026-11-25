# frozen_string_literal: true

require "test_helper"

# E2E — what a sandbox says its runs are doing now, read while one of them
# waits on a directory another sandbox's run holds.
class TestE2ESandboxStat < Minitest::Test
  include E2eGuestHelper
  include Handshake

  def setup
    @tmpfs = Kobako::Filesystem::Tmpfs.new
    @holding = Queue.new
    @release = Queue.new
  end

  def teardown
    @release << true
    @holder&.join
  end

  # @behavior S-264
  def test_a_sandbox_counts_its_run_waiting_on_a_directory_until_it_ends
    hold_directory
    looker = sandbox_on
    looking = Thread.new { looker.eval('File.exist?("/data/new")').value }
    wait_until("the looker never waited on the directory") { looker.stat(:runs_waiting) == 1 }
    waiting = looker.stat
    @release << true

    assert_equal [{ runs_waiting: 1 }, true, 0], [waiting, looking.value, looker.stat(:runs_waiting)],
                 "Sandbox#stat must count a run waiting on a directory another run holds, and " \
                 "Sandbox#stat(:runs_waiting) no run once it ends"
  end

  private

  # Start a run creating /data/new whose mount's block waits on the release,
  # holding the directory, once the block says it began.
  def hold_directory
    @holder = Thread.new { sandbox_on(holds: true).eval('File.write("/data/new", "x")') }
    wait_until("the holder never reached its mount's block holding the directory") { !@holding.empty? }
  end

  # A sandbox over the +Tmpfs+ at /data whose block, on a change when +holds+,
  # says so and waits on the release.
  def sandbox_on(holds: false)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", @tmpfs, access: :read_write) do |_path, access|
      if holds && access == :read_write
        @holding << true
        @release.pop
      end
      true
    end
  end
end
