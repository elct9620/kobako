# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Pool under cross-thread contention — the blocking
# checkout wait, its timeout bound, and checkout independence, driving the
# real data/kobako.wasm.
class TestPoolContention < Minitest::Test
  include E2eGuestHelper

  # @behavior PL-024
  # The wait is bounded so a full Pool refuses rather than hanging the
  # caller; the refusal has to arrive before any Sandbox is touched.
  def test_exhausted_pool_times_out
    pool = Kobako::Pool.new(size: 1, checkout_timeout: 0.05)
    release, holder = hold_one_slot(pool)
    assert_raises(Kobako::PoolTimeoutError,
                  "a checkout past checkout_timeout on a full pool must raise PoolTimeoutError") do
      pool.with { |sandbox| sandbox }
    end
    release << true
    holder.join
  end

  # @behavior PL-011
  def test_blocked_checkout_proceeds_on_checkin
    pool = Kobako::Pool.new(size: 1, checkout_timeout: 5.0)
    release, holder = hold_one_slot(pool)
    waiter = Thread.new { pool.with { |sandbox| sandbox.eval("3").value } }
    Thread.pass until waiter.stop?
    release << true
    assert_equal 3, waiter.value, "a blocked checkout must receive the checked-in Sandbox and proceed"
    holder.join
  end

  # @behavior PL-012
  def test_nested_with_checks_out_a_distinct_sandbox
    pool = Kobako::Pool.new(size: 2)
    pool.with do |outer|
      pool.with do |inner|
        refute_same outer, inner, "a nested Pool#with on the same thread must hold a distinct Sandbox"
      end
    end
  end

  # @behavior PL-036
  def test_a_waiter_interrupted_as_it_is_woken_passes_the_slot_on
    pool = Kobako::Pool.new(size: 1, checkout_timeout: 2.0)

    answer, waited = checkout_behind_one_interrupted_as_woken(pool)

    assert answer == :second && waited < 1.0,
           "a Pool#with checkout waiting behind one interrupted as the slot is checked in must receive " \
           "the Sandbox at once, not at checkout_timeout; waited #{waited.round(2)}s"
  end

  # @behavior PL-037
  def test_an_interrupt_as_a_checkout_ends_keeps_its_sandbox_in_the_pool
    pool = Kobako::Pool.new(size: 1, checkout_timeout: 0.5)
    pool.with { |sandbox| sandbox }

    assert_raises(Interrupt, "an interrupt arriving as Pool#with takes its Sandbox must be raised by Pool#with") do
      interrupt_as_a_checkout_claims.enable { pool.with { flunk "the block must not run once interrupted" } }
    end
    assert_equal 2, pool.with { |sandbox| sandbox.eval("1 + 1").value },
                 "the next Pool#with checkout must receive the Sandbox the interrupted checkout took"
  end

  private

  # Check out the pool's only Sandbox on a background thread and hold it
  # until the returned release queue receives a value; returns after the
  # hold is observably in place.
  # Hold the only slot on another thread until the returned queue is
  # pushed; +after_checkin+ runs on that thread once it checks back in.
  def hold_one_slot(pool, &after_checkin)
    held = Queue.new
    release = Queue.new
    holder = Thread.new do
      pool.with { held.push(true).then { release.pop } }
      after_checkin&.call
    end
    held.pop
    [release, holder]
  end

  # The answer and wait of a checkout queued behind one the holder
  # interrupts right as it checks the slot back in.
  def checkout_behind_one_interrupted_as_woken(pool)
    first = nil
    release, holder = hold_one_slot(pool) { first.raise(Interrupt) }
    first = waiting { pool.with { :first } }
    second = waiting { timed { pool.with { :second } } }
    release << true
    [holder, first].each(&:join)
    second.value
  end

  # A thread blocked in +checkout+, answering :interrupted if interrupted.
  def waiting(&checkout)
    thread = Thread.new do
      checkout.call
    rescue Interrupt
      :interrupted
    end
    Thread.pass until thread.stop?
    thread
  end

  def timed
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    [yield, Process.clock_gettime(Process::CLOCK_MONOTONIC) - started]
  end

  # Raise an interrupt on the checking-out thread once, as the checkout
  # takes its Sandbox.
  def interrupt_as_a_checkout_claims
    TracePoint.new(:return) do |point|
      next unless point.method_id == :claim_or_wait

      point.disable
      Thread.current.raise(Interrupt)
    end
  end
end
