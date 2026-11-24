# frozen_string_literal: true

require "test_helper"

# E2E — a run taking all the memory or the time it may beside another run
# on another Thread, under both gvl: modes: the other run still reaches its
# own memory_limit and answers.
class TestE2EGvlMemoryNeighbors < Minitest::Test
  include E2eGuestHelper

  GVL_MODES = %i[hold release].freeze

  # Grows 3 MB of the default 4 MiB, raises and rescues, then grows 2 MB
  # more than the memory left holds.
  GREEDY = <<~GUEST
    kept = []
    3.times { kept << ("x" * 1_000_000) }
    begin; raise "any"; rescue StandardError; end
    2.times { kept << ("x" * 1_000_000) }
    kept.size
  GUEST

  # Grows 3 MB of the default 4 MiB and answers what it holds.
  VICTIM = 'kept = []; 3.times { kept << ("x" * 1_000_000) }; kept.sum(&:bytesize)'

  # @behavior RT-418
  def test_a_run_filling_its_memory_leaves_a_neighbour_its_own
    answers = GVL_MODES.map { |mode| side_by_side(mode) }

    assert_equal [[Kobako::MemoryLimitError, 3_000_000]] * 2, answers,
                 "a run filling its memory_limit and refused more growth must leave a run beside it " \
                 "on another Thread its own memory_limit under :hold and :release"
  end

  # @behavior RT-419
  def test_a_run_filling_entropy_delays_a_neighbour_less_than_its_deadline
    spent = GVL_MODES.map { |mode| eval_beside_flood(mode) }

    assert spent.all? { |elapsed| elapsed < 1.0 },
           "a run beside one reading 64 MiB of entropy over and over under a 2 s timeout must answer " \
           "within a second under :hold and :release; it took #{spent.map { |s| s.round(2) }}"
  end

  private

  # The seconds from the flood's head start until a short script answers.
  # Both Sandboxes are built first, so the first compile of the Guest Binary
  # in a process with a cold artifact cache is not judged. The clock is read
  # before the head start, so the wait for the GVL to come back is.
  def eval_beside_flood(mode)
    neighbour = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode)
    flooder = Kobako::Sandbox.new(wasm_path: TestPaths.fixture("minimal_random_flood.wat"),
                                  profile: :permissive, gvl: mode, timeout: 2, memory_limit: nil)
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    flooding = Thread.new { flood(flooder) }
    sleep 0.05
    neighbour.eval("1")
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
    flooding.join
    elapsed
  end

  def side_by_side(mode)
    greedy = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode)
    victim = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode)
    [Thread.new { refusal(greedy) }, Thread.new { victim.eval(VICTIM).value }].map(&:value)
  end

  def refusal(sandbox)
    sandbox.eval(GREEDY)
    nil
  rescue Kobako::MemoryLimitError => e
    e.class
  end

  def flood(sandbox)
    sandbox.eval("nil")
  rescue Kobako::TimeoutError
    nil
  end
end
