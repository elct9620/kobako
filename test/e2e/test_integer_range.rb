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
  OVER_I64 = 9_223_372_036_854_775_808

  # @behavior CD-001
  # Saturating to the nearest bound would hand the script a different
  # number than the wire carried, with nothing to say it had changed.
  def test_service_return_above_i64_range_is_refused_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Clock::Millis", -> { OVER_I64 })

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("Clock::Millis.call") }

    assert_equal "Kobako::Transport::Error", err.klass,
                 "a Service return above the 64-bit range the guest carries through Sandbox#eval must " \
                 "attribute to the wire-level class, since the exchange is what did not complete"
    assert_match(/9223372036854775808.*64-bit range the guest carries/, err.message,
                 "the refusal must name the value it could not hold, not just that one failed")
  end

  # @behavior CD-002
  # Pinned against the refusal above so a check that over-reached by one
  # would be caught rather than read as caution.
  def test_service_return_at_i64_max_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Clock::Max", -> { I64_MAX })

    assert_equal I64_MAX, sandbox.eval("Clock::Max.call").value,
                 "an inbound integer at the 64-bit ceiling must round-trip, not be refused"
  end

  # @behavior CD-003
  # The refusal has to land at guest entry rather than at the entrypoint,
  # or the entrypoint would run against a value it was never sent.
  def test_run_argument_above_i64_range_fails_the_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Echo = ->(x) { x }", name: :Echo)

    err = assert_raises(Kobako::SandboxError) { sandbox.run(:Echo, OVER_I64) }

    assert_equal "Kobako::Transport::Error", err.klass,
                 "a #run argument above the 64-bit range the guest carries must fail the invocation at " \
                 "the wire level, before the entrypoint sees a saturated value"
    assert_match(/9223372036854775808.*64-bit range the guest carries/, err.message,
                 "the Panic is the only account of why the invocation never started, so it " \
                 "must say which value stopped it rather than that decoding failed")
  end

  # @behavior CD-004
  # The third inbound direction, and the only one whose refusal takes
  # three hops: the block never runs, so the guest answers the Yield Reply
  # with its error arm, the host re-raises that at the Service's own yield
  # site, and the Service raising is what the script finally sees.
  def test_yield_argument_above_i64_range_is_refused_at_the_yield_site
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Feed", ->(&blk) { blk.call(OVER_I64) })

    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval("Probe::Feed.call { |x| x }")
    end

    assert_match(/Transport::Error.*64-bit range the guest carries/, err.message,
                 "a yield argument above the 64-bit range the guest carries through Sandbox#eval must " \
                 "reach the script naming both the hop that refused and the range it " \
                 "exceeded, since the block never runs to report anything else")
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

  # @behavior MR-026
  # The parser hands a literal past 32 bits to mruby-bigint, which must keep
  # every digit rather than raise or round.
  def test_integer_literal_past_i64_range_stays_exact_inside_the_guest
    digits = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval("18446744073709551617.to_s").value

    assert_equal "18446744073709551617", digits,
                 "the literal 18446744073709551617 through Sandbox#eval must read back with every digit"
  end
end
