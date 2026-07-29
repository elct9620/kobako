# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — host→guest integer representability through real mruby
# (docs/wire/payload-msgpack.md § Integer Range). The guest is built MRB_INT32, so a
# wire integer outside the signed 32-bit range has no faithful guest
# representation. The guest refuses such a value rather than saturating it
# to the nearest bound, so the script never receives a different number
# than the wire carried.
class TestE2EIntegerRange < Minitest::Test
  include E2eGuestHelper

  I32_MAX = (2**31) - 1
  OVER_I32 = 2**31

  # dispatch-return path: a Service returning an integer beyond the guest's
  # range raises in the calling guest code rather than handing the script a
  # saturated value.
  def test_service_return_above_i32_range_is_refused_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Clock::Millis", -> { OVER_I32 })

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("Clock::Millis.call") }

    assert_equal "Kobako::Transport::Error", err.klass,
                 "a Service return above the guest's 32-bit range through Sandbox#eval must " \
                 "attribute to the wire-level class, since the exchange is what did not complete"
    assert_match(/2147483648.*32-bit Integer range/, err.message,
                 "the refusal must name the value it could not hold, not just that one failed")
  end

  # boundary guard: the largest in-range value still round-trips, so the
  # refusal does not over-reach.
  def test_service_return_at_i32_max_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Clock::Max", -> { I32_MAX })

    assert_equal I32_MAX, sandbox.eval("Clock::Max.call").value,
                 "an inbound integer at the 32-bit ceiling must round-trip, not be refused"
  end

  # E-26: a #run argument beyond the guest's range has no faithful guest
  # representation, so the invocation fails at guest entry rather than
  # reaching the entrypoint with a saturated value.
  def test_run_argument_above_i32_range_fails_the_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Echo = ->(x) { x }", name: :Echo)

    err = assert_raises(Kobako::SandboxError) { sandbox.run(:Echo, OVER_I32) }

    assert_equal "Kobako::Transport::Error", err.klass,
                 "a #run argument above the guest's 32-bit range must fail the invocation at " \
                 "the wire level, before the entrypoint sees a saturated value"
    assert_match(/2147483648.*32-bit Integer range/, err.message,
                 "the Panic is the only account of why the invocation never started, so it " \
                 "must say which value stopped it rather than that decoding failed")
  end

  # The third inbound direction, and the only one whose refusal takes
  # three hops: the block never runs, so the guest answers the Yield Reply
  # with its error arm, the host re-raises that at the Service's own yield
  # site, and the Service raising is what the script finally sees.
  def test_yield_argument_above_i32_range_is_refused_at_the_yield_site
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Feed", ->(&blk) { blk.call(OVER_I32) })

    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval("Probe::Feed.call { |x| x }")
    end

    assert_match(/Transport::Error.*32-bit Integer range/, err.message,
                 "a yield argument above the guest's 32-bit range through Sandbox#eval must " \
                 "reach the script naming both the hop that refused and the range it " \
                 "exceeded, since the block never runs to report anything else")
  end
end
