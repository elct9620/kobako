# frozen_string_literal: true

require "test_helper"

# Coverage for the Kobako::Pool checkout / checkin cycle on a single
# thread (docs/behavior.md B-46 / B-47), driving the real
# data/kobako.wasm.
class TestPoolCheckout < Minitest::Test
  include E2eGuestHelper

  # B-47
  def test_with_returns_block_value
    pool = Kobako::Pool.new(slots: 1)
    assert_equal 42, pool.with { |sandbox| sandbox.eval("40 + 2") },
                 "the block's return value through Pool#with must be returned to the caller (B-47)"
  end

  # B-46: a checkout prefers an idle Sandbox — the setup block runs once
  # per constructed Sandbox, not once per checkout.
  def test_sequential_checkouts_reuse_the_idle_sandbox
    constructed = []
    pool = Kobako::Pool.new(slots: 2) { |sandbox| constructed << sandbox }
    first = pool.with { |sandbox| sandbox }
    pool.with do |sandbox|
      assert_same first, sandbox, "a checkout with an idle Sandbox available must reuse it (B-46 idle-first)"
    end
    assert_equal 1, constructed.size, "the setup block must run exactly once per constructed Sandbox (B-46)"
  end

  # B-46 / B-47: the setup block's registrations persist across checkouts.
  def test_setup_block_registrations_visible_to_later_checkouts
    echo = Class.new do
      def call(value) = value
    end
    pool = Kobako::Pool.new(slots: 1) { |sandbox| sandbox.define(:Pooled).bind(:Echo, echo.new) }
    pool.with { |sandbox| sandbox.eval("Pooled::Echo.call(1)") }
    assert_equal 2, pool.with { |sandbox| sandbox.eval("Pooled::Echo.call(2)") },
                 "setup-block Service bindings through a reused pooled Sandbox must stay active (B-47)"
  end

  # B-47: output buffers read empty at checkout.
  def test_checkout_hands_over_empty_output_buffers
    pool = Kobako::Pool.new(slots: 1)
    pool.with { |sandbox| sandbox.eval(%(puts "leak?")) }
    pool.with do |sandbox|
      assert_equal "", sandbox.stdout, "a pooled Sandbox at checkout must read empty stdout (B-47)"
      assert_equal "", sandbox.stderr, "a pooled Sandbox at checkout must read empty stderr (B-47)"
      refute_predicate sandbox, :stdout_truncated?,
                       "a pooled Sandbox at checkout must read stdout_truncated? false (B-47)"
      refute_predicate sandbox, :stderr_truncated?,
                       "a pooled Sandbox at checkout must read stderr_truncated? false (B-47)"
    end
  end

  # B-47: no guest-observable state crosses from one checkout holder to
  # the next. The B-49 canonical-boot e2e pins this on a directly
  # constructed Sandbox; this is the Pool-composition witness on the
  # same global probe.
  def test_checkout_isolates_guest_global_state
    pool = Kobako::Pool.new(slots: 1)
    pool.with { |sandbox| sandbox.eval("$leak = 1") }
    pool.with do |sandbox|
      assert_nil sandbox.eval("$leak"),
                 "a guest global set in one checkout must read nil in the next checkout (B-47)"
    end
  end

  # B-46: Sandbox keywords forward verbatim — the forwarded timeout cap
  # governs invocations on pooled Sandboxes.
  def test_sandbox_keywords_forward_to_pooled_sandboxes
    pool = Kobako::Pool.new(slots: 1, timeout: 0.05)
    assert_raises(Kobako::TimeoutError, "an over-deadline eval through a pooled Sandbox must raise TimeoutError") do
      pool.with { |sandbox| sandbox.eval("loop do end") }
    end
  end
end
