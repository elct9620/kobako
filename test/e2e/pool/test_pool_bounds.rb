# frozen_string_literal: true

require "test_helper"

# Coverage for what a Kobako::Pool slot costs and when it is paid, driving
# the real data/kobako.wasm: a nested checkout holds a slot like any other
# holder, and a checkout that waits past its bound fails as itself and
# touches nothing.
class TestPoolBounds < Minitest::Test
  include E2eGuestHelper

  # @behavior PL-027
  def test_a_nested_checkout_waits_on_a_pool_whose_every_slot_is_held
    pool = Kobako::Pool.new(slots: 1, checkout_timeout: 0.05)

    assert_raises(Kobako::PoolTimeoutError,
                  "a nested Pool#with on a Pool whose only slot the outer checkout holds must wait") do
      pool.with { pool.with { |inner| inner } }
    end
  end

  # @behavior PL-028
  def test_a_checkout_past_its_bound_fails_as_none_of_the_invocation_outcomes
    pool = Kobako::Pool.new(slots: 1, checkout_timeout: 0.05)

    err = pool.with { assert_raises(Kobako::PoolTimeoutError) { pool.with { nil } } }

    [Kobako::TrapError, Kobako::SandboxError, Kobako::ServiceError].each do |outcome|
      refute_kind_of outcome, err, "a Pool checkout timeout must not be rescued as #{outcome}"
    end
  end

  # @behavior PL-029
  # The waiter timed out while the holder's run had written guest state the
  # holder can still read, so the timeout disturbed nothing it held.
  def test_a_timed_out_checkout_leaves_the_held_sandbox_as_its_holder_left_it
    pool = Kobako::Pool.new(slots: 1, checkout_timeout: 0.05)

    held, after = pool.with do |sandbox|
      sandbox.bind("Note::Read", -> { :held })
      assert_raises(Kobako::PoolTimeoutError) { pool.with { nil } }
      [sandbox, sandbox.eval("Note::Read.call").value]
    end

    assert_equal :held, after, "the holder must still drive its Sandbox after another checkout timed out"
    pool.with { |sandbox| assert_same held, sandbox, "a timed-out checkout must leave the pooled Sandbox in place" }
  end
end
