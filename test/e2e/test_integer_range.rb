# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — host→guest integer representability through real mruby
# (docs/wire-codec.md § Integer Range). The guest is built MRB_INT32, so a
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
    sandbox.define(:Clock).bind(:Millis, -> { OVER_I32 })

    assert_raises(Kobako::SandboxError,
                  "a Service return above the guest's 32-bit range must be refused, not saturated") do
      sandbox.eval("Clock::Millis.call")
    end
  end

  # boundary guard: the largest in-range value still round-trips, so the
  # refusal does not over-reach.
  def test_service_return_at_i32_max_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Clock).bind(:Max, -> { I32_MAX })

    assert_equal I32_MAX, sandbox.eval("Clock::Max.call"),
                 "an inbound integer at the 32-bit ceiling must round-trip, not be refused"
  end

  # #run argument path: an argument beyond the guest's range fails the
  # invocation rather than reaching the entrypoint with a saturated value.
  def test_run_argument_above_i32_range_fails_the_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Echo = ->(x) { x }", name: :Echo)

    assert_raises(Kobako::SandboxError,
                  "a #run argument above the guest's 32-bit range must fail the invocation") do
      sandbox.run(:Echo, OVER_I32)
    end
  end
end
