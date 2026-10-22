# frozen_string_literal: true

require "test_helper"

# E2E — host→guest integer representability through real mruby
# (docs/wire/payload-msgpack.md § Integer Range). An Integer crosses the
# wire only within the signed 64-bit range, so the guest refuses a wire
# integer outside it rather than saturating it to the nearest bound, and
# the script never receives a different number than the wire carried.
class TestE2EIntegerRange < Minitest::Test
  include E2eGuestHelper

  I64_MAX = 9_223_372_036_854_775_807

  # @behavior CD-002
  # Pinned against the refusal above so a check that over-reached by one
  # would be caught rather than read as caution.
  def test_service_return_at_i64_max_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Clock::Max", -> { I64_MAX })

    assert_equal I64_MAX, sandbox.eval("Clock::Max.call").value,
                 "an inbound integer at the 64-bit ceiling must round-trip, not be refused"
  end

  # @behavior CD-053
  # The guest holds an Integer of any size, so the bound it meets on the
  # way out is the wire's, refused like any value with no wire form.
  def test_guest_integer_past_i64_range_does_not_cross_to_the_host
    err = assert_raises(Kobako::SandboxError) do
      Kobako::Sandbox.new(wasm_path: REAL_WASM).eval("2**64")
    end

    assert_equal "return value of type Integer is not a supported sandbox value type", err.message,
                 "an Integer of 2**64 answered through Sandbox#eval must fail the invocation " \
                 "as a value the wire does not carry"
  end

  # @behavior CD-054
  # A known limit, not a design: mruby normalizes a big integer only when
  # its magnitude fits the positive range, so -2**63 stays big and the
  # guest's 64-bit read does not reach it.
  def test_smallest_i64_computed_from_a_big_integer_does_not_cross
    err = assert_raises(Kobako::SandboxError) do
      Kobako::Sandbox.new(wasm_path: REAL_WASM).eval("-(2**63)")
    end

    assert_equal "return value of type Integer is not a supported sandbox value type", err.message,
                 "-(2**63) answered through Sandbox#eval must fail the invocation as a value the " \
                 "wire does not carry, until the guest can read a big integer as 64 bits"
  end

  # @behavior MR-026
  # The parser hands a literal past 32 bits to mruby-bigint, which must keep
  # every digit rather than raise or round.
  def test_integer_literal_past_i64_range_stays_exact_inside_the_guest
    digits = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval("18446744073709551617.to_s").value

    assert_equal "18446744073709551617", digits,
                 "the literal 18446744073709551617 through Sandbox#eval must read back with every digit"
  end
end
