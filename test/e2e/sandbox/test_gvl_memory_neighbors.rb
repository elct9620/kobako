# frozen_string_literal: true

require "test_helper"

# E2E — a run taking all the memory or the time it may beside another run
# on another thread, under both gvl: modes: the other run still reaches its
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

  # Far past any slowdown, so a flood still running then means its neighbour
  # jammed; the flood ending first turns the test red.
  JAMMED_AFTER = 30

  # Ends the flood once the neighbour answered, so no run waits out a deadline.
  class Stop < StandardError
  end

  # @behavior RT-418
  def test_a_run_filling_its_memory_leaves_a_neighbour_its_own
    answers = GVL_MODES.map { |mode| eval_beside_greedy(mode) }

    assert_equal [[Kobako::MemoryLimitError, 3_000_000]] * 2, answers,
                 "a run filling its memory_limit and refused more growth must leave a run beside it " \
                 "on another Thread its own memory_limit under :hold and :release"
  end

  # @behavior RT-419
  def test_a_run_filling_entropy_leaves_a_neighbour_to_answer_while_it_floods
    answers = GVL_MODES.map { |mode| [mode, *eval_beside_flood(mode)] }

    assert_equal GVL_MODES.map { |mode| [mode, 1, true] }, answers,
                 "a run beside one reading 64 MiB of entropy over and over must answer while the flood " \
                 "still runs, under :hold and :release"
  end

  private

  # What a short script answers beside the flood, and whether the flood still
  # ran as it answered. Both sandboxes are built first, so the first compile of
  # the guest binary in a process with a cold artifact cache plays no part.
  def eval_beside_flood(mode)
    neighbour = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode)
    flooder = Kobako::Sandbox.new(wasm_path: TestPaths.fixture("minimal_random_flood.wat"),
                                  profile: :permissive, gvl: mode, timeout: JAMMED_AFTER, memory_limit: nil)
    flooding = Thread.new { flood(flooder) }
    sleep 0.05
    answer = neighbour.eval("1").value
    still = flooding.alive?
    flooding.raise(Stop)
    flooding.join
    [answer, still]
  end

  def eval_beside_greedy(mode)
    greedy = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode)
    victim = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode)
    [Thread.new { eval_greedy(greedy) }, Thread.new { victim.eval(VICTIM).value }].map(&:value)
  end

  def eval_greedy(sandbox)
    sandbox.eval(GREEDY)
    nil
  rescue Kobako::MemoryLimitError => e
    e.class
  end

  def flood(sandbox)
    sandbox.eval("nil")
  rescue Kobako::TimeoutError, Stop
    nil
  end
end
