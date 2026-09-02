# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Pool construction pre-flight
# (docs/behavior/runtime.md B-46 + E-47). Pool.new builds no Sandbox, so every
# case here runs without the native ext.
class TestPoolConstruction < Minitest::Test
  # @behavior PL-020
  # A slot count is what the Pool sizes itself by, so a value it cannot
  # count with is refused at construction rather than at the first
  # checkout, where the caller would already be waiting on it.
  def test_e47_slots_must_be_positive_integer
    [0, -1, 1.5, "3", nil].each do |bad|
      err = assert_raises(ArgumentError, "slots: #{bad.inspect} through Pool.new must raise ArgumentError") do
        Kobako::Pool.new(slots: bad)
      end
      assert_match(/slots/, err.message)
    end
  end

  # @behavior PL-021
  # Every rejected form here is one that would make the wait unbounded
  # by accident rather than by request — the deliberate unbounded wait
  # has its own spelling, pinned in the next test.
  def test_e47_checkout_timeout_must_be_positive_finite_or_nil
    [0, -1, Float::INFINITY, Float::NAN, "5"].each do |bad|
      err = assert_raises(ArgumentError,
                          "checkout_timeout: #{bad.inspect} through Pool.new must raise ArgumentError") do
        Kobako::Pool.new(slots: 1, checkout_timeout: bad)
      end
      assert_match(/checkout_timeout/, err.message)
    end
  end

  # @behavior PL-022
  # The sentinel that selects an indefinite wait has to construct, or
  # the only way to ask for one would be a value the checks reject.
  def test_e47_nil_checkout_timeout_is_valid
    assert_instance_of Kobako::Pool, Kobako::Pool.new(slots: 1, checkout_timeout: nil),
                       "checkout_timeout: nil through Pool.new must construct a Pool (E-47)"
  end

  # @behavior PL-002
  # B-46: the default checkout wait bound is 5.0 seconds. Pinned on the
  # public constant — the keyword default consumes it, and a timed
  # behavioral witness would cost the suite a 5-second wait.
  def test_checkout_timeout_defaults_to_five_seconds
    assert_in_delta 5.0, Kobako::Pool::DEFAULT_CHECKOUT_TIMEOUT_SECONDS, 0.0,
                    "Pool.new without checkout_timeout: must bound the wait at 5.0 seconds (B-46)"
  end

  # @behavior PL-001
  # B-46: Pool.new constructs no Sandbox — construction is checkout-driven.
  def test_construction_is_lazy
    setup_runs = 0
    Kobako::Pool.new(slots: 2) { |_sandbox| setup_runs += 1 }
    assert_equal 0, setup_runs, "Pool.new must not construct any Sandbox before the first checkout (B-46)"
  end

  # @behavior PL-023
  # A checkout timeout reaches the Host App from a different place than
  # an invocation outcome does, so a single `rescue Kobako::Error` has to
  # cover both or the two would need separate handling.
  def test_pool_timeout_error_sits_under_kobako_error
    assert_operator Kobako::PoolTimeoutError, :<, Kobako::Error,
                    "Kobako::PoolTimeoutError must be rescuable as Kobako::Error"
  end
end
