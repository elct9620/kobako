# frozen_string_literal: true

require "test_helper"

# E2E — a run taking all the memory or the time it may beside another run
# on another thread, under both gvl: modes: the other run still reaches its
# own memory_limit and answers.
class TestE2EGvlMemoryNeighbors < Minitest::Test
  include E2eGuestHelper
  include Handshake

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
  MAX_RUN_TIME = 30

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

  # What a short script answers beside the flood, once the flood began, and
  # whether the flood still ran as it answered. Both sandboxes are built first,
  # so the first compile of the guest binary in a process with a cold artifact
  # cache plays no part.
  def eval_beside_flood(mode)
    neighbour = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode)
    begun = Thread::Queue.new
    flooder = flooding_sandbox(mode, begun)
    flooding = Thread.new { flood(flooder) }
    wait_until("the flood never opened /data, so its run failed first") { !begun.empty? || !flooding.alive? }
    answer = neighbour.eval("1").value
    still = flooding.alive?
    flooding.raise(Stop)
    flooding.join
    [answer, still]
  end

  # A sandbox whose run floods entropy under +mode+, filling +begun+ once
  # its guest opened /data on the way to the flood.
  def flooding_sandbox(mode, begun)
    Kobako::Sandbox.new(wasm_path: TestPaths.fixture("minimal_random_flood.wat"),
                        profile: :permissive, gvl: mode, timeout: MAX_RUN_TIME, memory_limit: nil)
                   .mount("/data", Kobako::Filesystem::Tmpfs.new, access: :read) { begun << true }
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
