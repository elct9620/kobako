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
    assert_raises(Kobako::MemoryLimitError,
                  "memory a yielded block grows through #eval must end the invocation as the budget's own trap") do
      yielding_sandbox(memory_limit: 1 << 20).eval(GROWING_BLOCK)
    end
  end

  # A trap leaves the guest mid-step, so a Service that shrugs off the
  # first failed yield must not resume it with a second.
  # @behavior T-264
  def test_a_guest_that_trapped_is_not_entered_again
    entries = 0
    sandbox = retrying_sandbox(memory_limit: 1 << 20)
    sandbox.bind("Probe::Enter", -> { entries += 1 })

    assert_raises(Kobako::MemoryLimitError) { sandbox.eval(TRAPPING_BLOCK) }

    assert_equal 1, entries, "a second yield after the guest trapped through #eval must not run the block again"
  end

  # @behavior T-276
  def test_a_service_rescuing_a_trapped_yield_does_not_hide_the_trap
    sandbox = retrying_sandbox(memory_limit: 1 << 20)
    sandbox.bind("Probe::Enter", -> {})

    assert_raises(Kobako::MemoryLimitError,
                  "a Service rescuing a trapped yield through #eval must not hide the trap") do
      sandbox.eval(TRAPPING_BLOCK)
    end
  end

  TRAPPING_BLOCK = "Probe::Yields.call { Probe::Enter.call; Array.new(4) { 'a' * 900_000 }.size }"

  private

  # A Service that yields twice, shrugging off whatever trap the first
  # yield raised.
  def retrying_sandbox(memory_limit:)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: memory_limit)
    sandbox.bind("Probe::Yields", lambda do |&blk|
      2.times do
        blk.call
      rescue Kobako::TrapError
        nil
      end
    end)
    sandbox
  end

  def yielding_sandbox(memory_limit:)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: memory_limit)
    sandbox.bind("Probe::Yields", ->(&blk) { blk.call })
    sandbox
  end
end
