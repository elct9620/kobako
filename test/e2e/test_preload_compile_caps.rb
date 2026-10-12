# frozen_string_literal: true

require "test_helper"

# E2E — the caps that bound an invocation also bound the compile of its
# preloaded source, and the compile spends none of the invocation's own.
# The real Guest Binary compiles too quickly to reach a cap, so the
# fixture's compile grows past the memory budget, then spins.
class TestE2EPreloadCompileCaps < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_runaway_compile.wat")
  DEADLINE = 0.3

  def setup
    require_fixture!(FIXTURE_PATH)
  end

  # The fixture's growth fits no budget, so only the deadline stops it.
  # @behavior S-192
  def test_a_compile_past_the_deadline_leaves_the_source_replaying
    elapsed, execution = timed_first_eval(timeout: DEADLINE, memory_limit: nil)

    assert_nil execution.value,
               "preloaded source whose compile runs past the deadline through Sandbox#eval must still " \
               "run the invocation on the source"
    assert_operator elapsed, :>=, DEADLINE,
                    "the compile through Sandbox#eval must run until the deadline stops it"
  end

  # A deadline far above the fixture's cost leaves the budget as the only
  # cap that can end the compile this soon.
  # @behavior S-192
  def test_a_compile_past_the_memory_budget_leaves_the_source_replaying
    elapsed, execution = timed_first_eval(timeout: 5, memory_limit: 1 << 20)

    assert_nil execution.value,
               "preloaded source whose compile grows past the memory budget through Sandbox#eval must " \
               "still run the invocation on the source"
    assert_operator elapsed, :<, 5,
                    "the compile through Sandbox#eval must be stopped by the memory budget, not the deadline"
  end

  # @behavior S-193
  def test_compiling_spends_none_of_the_first_invocations_deadline
    _, execution = timed_first_eval(timeout: DEADLINE, memory_limit: nil)

    assert_operator execution.usage.wall_time, :<, DEADLINE,
                    "a first Sandbox#eval after a compile that used up the deadline must complete and " \
                    "report a wall time below that deadline"
  end

  private

  def timed_first_eval(**caps)
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, **caps)
    sandbox.preload(code: "A = 1", name: :A)
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    execution = sandbox.eval("A")
    [Process.clock_gettime(Process::CLOCK_MONOTONIC) - started, execution]
  end
end
