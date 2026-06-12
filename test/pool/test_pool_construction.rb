# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Pool construction pre-flight
# (docs/behavior.md B-46 + E-47). Pool.new builds no Sandbox, so every
# case here runs without the native ext.
class TestPoolConstruction < Minitest::Test
  # E-47
  def test_e47_slots_must_be_positive_integer
    [0, -1, 1.5, "3", nil].each do |bad|
      err = assert_raises(ArgumentError, "slots: #{bad.inspect} through Pool.new must raise ArgumentError") do
        Kobako::Pool.new(slots: bad)
      end
      assert_match(/slots/, err.message)
    end
  end

  # E-47: nil is valid (waits indefinitely); zero, negative, and
  # non-finite values are not.
  def test_e47_checkout_timeout_must_be_positive_finite_or_nil
    [0, -1, Float::INFINITY, Float::NAN, "5"].each do |bad|
      err = assert_raises(ArgumentError,
                          "checkout_timeout: #{bad.inspect} through Pool.new must raise ArgumentError") do
        Kobako::Pool.new(slots: 1, checkout_timeout: bad)
      end
      assert_match(/checkout_timeout/, err.message)
    end
  end

  # E-47: the nil sentinel constructs — it selects the indefinite wait.
  def test_e47_nil_checkout_timeout_is_valid
    assert_instance_of Kobako::Pool, Kobako::Pool.new(slots: 1, checkout_timeout: nil),
                       "checkout_timeout: nil through Pool.new must construct a Pool (E-47)"
  end

  # B-46: Pool.new constructs no Sandbox — construction is checkout-driven.
  def test_construction_is_lazy
    setup_runs = 0
    Kobako::Pool.new(slots: 2) { |_sandbox| setup_runs += 1 }
    assert_equal 0, setup_runs, "Pool.new must not construct any Sandbox before the first checkout (B-46)"
  end

  # E-46 taxonomy: a single `rescue Kobako::Error` covers pool checkout
  # timeouts alongside the invocation-outcome classes.
  def test_pool_timeout_error_sits_under_kobako_error
    assert_operator Kobako::PoolTimeoutError, :<, Kobako::Error,
                    "Kobako::PoolTimeoutError must be rescuable as Kobako::Error"
  end
end
