# frozen_string_literal: true

require "test_helper"

# E2E — a yield spends the invocation's own budgets, through real mruby.
# The block runs inside the invocation that called the Service, so the
# time spent in it and around it counts against that invocation's
# deadline, and the memory it grows against that invocation's budget.
class TestE2EYieldBudget < Minitest::Test
  include E2eGuestHelper

  # The Service sleeps past the deadline before yielding to a block that
  # does nothing, so only the time spent around the yield can end the run.
  # @behavior T-248
  def test_time_in_and_around_a_yield_counts_against_the_deadline
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.05)
    sandbox.bind("Probe::Yields", lambda do |&blk|
      sleep 0.2
      blk.call
    end)

    assert_raises(Kobako::TimeoutError,
                  "time spent in a yielded block and the Service around it must count against the deadline") do
      sandbox.eval("Probe::Yields.call { :block }")
    end
  end

  GROWING_BLOCK = "Probe::Yields.call { Array.new(4) { 'a' * 900_000 }.size }"

  # The same block fits a roomier budget, so the trap is the budget's.
  # @behavior T-249
  def test_memory_a_yielded_block_grows_counts_against_the_budget
    assert_equal 4, yielding_sandbox(memory_limit: 64 << 20).eval(GROWING_BLOCK).value,
                 "the block must fit a budget large enough for what it grows"
    assert_raises(Kobako::TrapError,
                  "memory a yielded block grows must count against the invocation's budget") do
      yielding_sandbox(memory_limit: 1 << 20).eval(GROWING_BLOCK)
    end
  end

  private

  def yielding_sandbox(memory_limit:)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: memory_limit)
    sandbox.bind("Probe::Yields", ->(&blk) { blk.call })
    sandbox
  end
end
