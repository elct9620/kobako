# frozen_string_literal: true

require "test_helper"

# E2E — preview1's +random_get+ as kobako answers it: the buffer is checked
# before any entropy is drawn, and a long fill stops at the run's deadline.
class TestE2ERandomGet < Minitest::Test
  # @behavior RT-416
  def test_entropy_asked_for_past_the_memory_traps_at_once
    sandbox = Kobako::Sandbox.new(wasm_path: TestPaths.fixture("minimal_random_outside.wat"), profile: :permissive)

    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    assert_raises(Kobako::TrapError) { sandbox.eval("nil") }
    spent = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started

    assert_operator spent, :<, 0.5,
                    "random_get into a 64 MiB buffer past a 64 KiB memory must trap before drawing " \
                    "any entropy; it took #{spent.round(2)}s"
  end

  # @behavior RT-417
  def test_a_run_reading_entropy_stops_at_its_deadline
    sandbox = Kobako::Sandbox.new(wasm_path: TestPaths.fixture("minimal_random_flood.wat"),
                                  profile: :permissive, timeout: 0.2, memory_limit: nil)

    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    assert_raises(Kobako::TimeoutError) { sandbox.eval("nil") }
    spent = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started

    assert_operator spent, :<, 1.0,
                    "a run reading 64 MiB of entropy over and over under a 0.2s timeout must raise " \
                    "TimeoutError within a second; it took #{spent.round(2)}s"
  end
end
