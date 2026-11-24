# frozen_string_literal: true

require "test_helper"

# E2E — what counts against +memory_limit+ from the moment a run's Store
# opens: what a Guest Binary declares is its baseline, and every growth past
# it, whichever memory, table, or heap it lands in, counts against the one
# cap.
class TestE2EMemoryBudget < Minitest::Test
  include E2eGuestHelper

  CAP = 1 << 20

  # Grows 3 MB of the default 4 MiB cap, rescues an exception, then grows
  # 2 MB more than the cap leaves.
  GROW_AFTER_RESCUE = <<~GUEST
    kept = []
    3.times { kept << ("x" * 1_000_000) }
    begin; raise "any"; rescue StandardError; end
    2.times { kept << ("x" * 1_000_000) }
    kept.size
  GUEST

  # A Guest Binary's declared memory and table, each past the cap, are its
  # baseline: the run gets past instantiation and fails only for answering
  # nothing.
  # @behavior S-113
  def test_what_a_guest_binary_declares_takes_none_of_the_memory_cap
    error = assert_raises(Kobako::TrapError) { run_fixture("minimal_declared_large.wat") }

    assert_equal Kobako::TrapError, error.class,
                 "a Guest Binary declaring a 2 MiB memory and a 2 MiB table under a 1 MiB memory_limit " \
                 "must fail for answering nothing, not as MemoryLimitError; got #{error.class}: #{error.message}"
  end

  # @behavior S-256
  def test_two_memories_growing_add_up_against_the_memory_cap
    assert_raises(Kobako::MemoryLimitError,
                  "two memories growing 768 KiB each under a 1 MiB memory_limit must raise " \
                  "MemoryLimitError") { run_fixture("minimal_two_memories.wat") }
  end

  # @behavior S-258
  def test_a_table_growing_past_the_memory_cap_raises
    assert_raises(Kobako::MemoryLimitError,
                  "a run growing a table by 2 MiB of entries under a 1 MiB memory_limit must raise " \
                  "MemoryLimitError") { run_fixture("minimal_growing_table.wat") }
  end

  # Building the Sandbox instantiates the guest outside any run, so it goes
  # through; only the run's own instantiation is charged.
  # @behavior S-259
  def test_a_start_function_growing_past_the_memory_cap_raises
    assert_raises(Kobako::MemoryLimitError,
                  "a start function growing memory by 2 MiB under a 1 MiB memory_limit must raise " \
                  "MemoryLimitError") { run_fixture("minimal_growing_start.wat") }
  end

  # wasmtime drops the budget's refusal to grow the GC heap and fails the
  # allocation instead; the run still ends as the budget's own failure.
  # @behavior S-259
  def test_a_collected_global_past_the_memory_cap_raises
    assert_raises(Kobako::MemoryLimitError,
                  "a global holding a 2 MiB GC array under a 1 MiB memory_limit must raise " \
                  "MemoryLimitError") { run_fixture("minimal_gc_global.wat") }
  end

  # The request alone fits; the copy the guest reads it into does not fit
  # beside it.
  # @behavior S-260
  def test_a_source_and_its_copy_past_the_memory_cap_raise
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: CAP)

    assert_raises(Kobako::MemoryLimitError,
                  "evaluating a 900 KB source under a 1 MiB memory_limit must raise MemoryLimitError") do
      sandbox.eval("#{"#" * 900_000}\nnil")
    end
  end

  # An exception lives in the engine's collected heap, apart from the linear
  # memory the script grew; both count, so the growth after it finds the cap
  # as full as a run that raised nothing does.
  # @behavior S-257
  def test_a_rescued_exception_leaves_the_memory_cap_as_full
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_raises(Kobako::MemoryLimitError,
                  "growing 2 MB more after growing 3 MB of a 4 MiB memory_limit and rescuing an exception " \
                  "must raise MemoryLimitError") { sandbox.eval(GROW_AFTER_RESCUE) }
  end

  private

  def run_fixture(name)
    Kobako::Sandbox.new(wasm_path: TestPaths.fixture(name), memory_limit: CAP).eval("nil")
  end
end
