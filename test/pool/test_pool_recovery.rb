# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Pool slot recovery — the checkin contract on a
# raising block (TrapError discards and recreates, anything else checks
# back in) and capacity release after a failed construction
# (docs/behavior/runtime.md B-46 / B-47), driving the real data/kobako.wasm.
class TestPoolRecovery < Minitest::Test
  include E2eGuestHelper

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

  # B-47: only TrapError discards — a guest exception surfaces as
  # SandboxError and leaves the Sandbox healthy, so checkin must return
  # it to the pool. A regression widening the discard rescue (or losing
  # the ensure-checkin) would rebuild or leak the slot on every guest
  # error while the TrapError-side test stays green.
  def test_non_trap_error_checks_the_sandbox_back_in
    constructed = []
    pool = Kobako::Pool.new(slots: 1) { |sandbox| constructed << sandbox }
    assert_raises(Kobako::SandboxError) { pool.with { |sandbox| sandbox.eval(%(raise "boom")) } }
    pool.with do |sandbox|
      assert_same constructed.first, sandbox,
                  "a SandboxError through Pool#with must check the same Sandbox back in, not discard it (B-47)"
    end
    assert_equal 1, constructed.size, "a non-TrapError block exit must not trigger a fresh construction (B-47)"
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
