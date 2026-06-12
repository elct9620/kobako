# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Pool slot recovery — the TrapError
# discard-and-recreate contract at checkin and capacity release after a
# failed construction (docs/behavior.md B-46 / B-47), driving the real
# data/kobako.wasm.
class TestPoolRecovery < Minitest::Test
  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
  end

  # B-47: a TrapError discards the Sandbox at checkin; the slot refills
  # with a fresh construction + setup-block run on next demand.
  def test_trap_error_discards_and_refills_the_slot
    constructed = []
    pool = Kobako::Pool.new(slots: 1, timeout: 0.05) { |sandbox| constructed << sandbox }
    assert_raises(Kobako::TimeoutError) { pool.with { |sandbox| sandbox.eval("loop do end") } }
    pool.with do |sandbox|
      refute_same constructed.first, sandbox,
                  "a checkout after a TrapError must never receive the unrecoverable Sandbox (B-47)"
      assert_equal 1, sandbox.eval("1"), "the refilled Sandbox must invoke normally (B-47)"
    end
    assert_equal 2, constructed.size, "the discarded slot must refill via a fresh construction (B-47)"
  end

  # B-46: a setup-block error surfaces at the triggering checkout and
  # releases the reserved slot capacity for a later retry.
  def test_setup_block_error_propagates_and_releases_capacity
    attempts = 0
    pool = Kobako::Pool.new(slots: 1) do |_sandbox|
      attempts += 1
      raise "setup boom" if attempts == 1
    end
    err = assert_raises(RuntimeError) { pool.with { |sandbox| sandbox } }
    assert_equal "setup boom", err.message
    assert_equal 2, pool.with { |sandbox| sandbox.eval("2") },
                 "a checkout after a failed construction must retry construction in the freed slot (B-46)"
  end
end
