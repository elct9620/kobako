# frozen_string_literal: true

require "test_helper"
require "tmpdir"

# E2E — the walk one run takes beside a neighbour holding each kind of thing
# a run can occupy, under both gvl: modes. Each tier's own scenario shows its
# rule alone; this one shows the rules compose, so the run arrives.
#
# A jam leaves no Thread of the process able to run a timeout, so each mode
# walks in a child the test kills past a deadline. Every neighbour is in
# place before the run starts and still there as it ends, so none can have
# finished early and left the run alone.
class TestE2EJourneysNeighbours < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  GVL_MODES = %i[hold release].freeze
  CHILD = File.read(TestPaths.fixture("journey_neighbours.rb")).freeze

  # Past any slowdown the neighbours cause, so reaching it means a jam.
  DEADLINE = 120

  EXPECTED = [["event", "files", "files", "files", "gvl", "leaf refused Errno::EAGAIN Errno::EMFILE"],
              ["hello", "ping\n", 3_000_000, true], ["inner\n"], :refused, ["hello"], []].inspect.freeze

  def setup
    super
    skip "Windows has no RLIMIT_NOFILE to lower" if Gem.win_platform?
    @host = Dir.mktmpdir("kobako-journey-neighbours")
    4.times do |at|
      FileUtils.mkdir_p(File.join(@host, "b#{at}", "d"))
      File.write(File.join(@host, "b#{at}", "d", "note.txt"), "hello")
    end
  end

  def teardown
    FileUtils.rm_rf(@host) if @host
    super
  end

  # @behavior J-018
  def test_a_run_beside_every_kind_of_occupant_finishes_its_own_work
    answers = GVL_MODES.map do |mode|
      [mode, run_ruby(CHILD, mode.to_s, @host, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE)]
    end

    assert_equal GVL_MODES.map { |mode| [mode, EXPECTED] }, answers,
                 "a run beside neighbours computing without end, parking a directory, waiting on a busy socket, " \
                 "and holding their descriptors, each at its own caps, must read a Bind, round trip, fill its " \
                 "memory, ask the parked directory, and run a nested run from its mount's block; a fork beside " \
                 "it must raise, every neighbour must still be there as it ends, and 12 runs after it must finish"
  end
end
