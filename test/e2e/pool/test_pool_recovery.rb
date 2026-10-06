# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Pool slot recovery — a raising block checks its
# Sandbox back in, and a failed construction releases its capacity,
# driving the real data/kobako.wasm.
class TestPoolRecovery < Minitest::Test
  include E2eGuestHelper

  # @behavior PL-032
  # A trap ends one invocation on its own instance, so the Sandbox that
  # met it serves the next checkout unchanged.
  def test_trap_error_checks_the_sandbox_back_in
    constructed = []
    pool = Kobako::Pool.new(slots: 1, timeout: 0.05) { |sandbox| constructed << sandbox }
    assert_raises(Kobako::TimeoutError) { pool.with { |sandbox| sandbox.eval("loop do end") } }

    value = pool.with do |sandbox|
      assert_same constructed.first, sandbox,
                  "a checkout after a TrapError through Pool#with must receive the Sandbox the trap left"
      sandbox.eval("1").value
    end
    assert_equal 1, value, "the Sandbox a trap left through Pool#with must evaluate guest code"
  end

  # @behavior PL-017
  # A guest exception leaves the Sandbox in the pool without a rebuild.
  def test_sandbox_error_checks_the_sandbox_back_in
    constructed = []
    pool = Kobako::Pool.new(slots: 1) { |sandbox| constructed << sandbox }
    assert_raises(Kobako::SandboxError) { pool.with { |sandbox| sandbox.eval(%(raise "boom")) } }
    pool.with { nil }

    assert_equal 1, constructed.size,
                 "a SandboxError through Pool#with must not cost a fresh construction on the next checkout"
  end

  # @behavior PL-006 PL-007
  # A setup-block error surfaces at the triggering checkout and releases
  # the reserved slot capacity for a later retry.
  def test_setup_block_error_propagates_and_releases_capacity
    attempts = 0
    pool = Kobako::Pool.new(slots: 1) do |_sandbox|
      attempts += 1
      raise "setup boom" if attempts == 1
    end
    err = assert_raises(RuntimeError) { pool.with { |sandbox| sandbox } }
    assert_equal "setup boom", err.message
    assert_equal 2, pool.with { |sandbox| sandbox.eval("2").value },
                 "a checkout after a failed construction must retry construction in the freed slot"
  end
end
