# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Pool under cross-thread contention — the blocking
# checkout wait, its E-46 timeout bound, and checkout independence
# (docs/behavior.md B-47 + E-46), driving the real data/kobako.wasm.
class TestPoolContention < Minitest::Test
  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
  end

  # E-46: all slots held past checkout_timeout raises PoolTimeoutError.
  def test_e46_exhausted_pool_times_out
    pool = Kobako::Pool.new(slots: 1, checkout_timeout: 0.05)
    release, holder = hold_one_slot(pool)
    assert_raises(Kobako::PoolTimeoutError, "a checkout past checkout_timeout on a full pool must raise E-46") do
      pool.with { |sandbox| sandbox }
    end
    release << true
    holder.join
  end

  # B-47: a blocked checkout proceeds as soon as a holder checks in.
  def test_blocked_checkout_proceeds_on_checkin
    pool = Kobako::Pool.new(slots: 1, checkout_timeout: 5.0)
    release, holder = hold_one_slot(pool)
    waiter = Thread.new { pool.with { |sandbox| sandbox.eval("3") } }
    release << true
    assert_equal 3, waiter.value, "a blocked checkout must receive the checked-in Sandbox and proceed (B-47)"
    holder.join
  end

  # B-47: checkouts are independent — a nested #with draws a second slot.
  def test_nested_with_checks_out_a_distinct_sandbox
    pool = Kobako::Pool.new(slots: 2)
    pool.with do |outer|
      pool.with do |inner|
        refute_same outer, inner, "a nested Pool#with on the same thread must hold a distinct Sandbox (B-47)"
      end
    end
  end

  private

  # Check out the pool's only Sandbox on a background thread and hold it
  # until the returned release queue receives a value; returns after the
  # hold is observably in place.
  def hold_one_slot(pool)
    held = Queue.new
    release = Queue.new
    holder = Thread.new do
      pool.with do |_sandbox|
        held << true
        release.pop
      end
    end
    held.pop
    [release, holder]
  end
end
