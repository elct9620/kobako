# frozen_string_literal: true

require "test_helper"

# E2E — the Regexp surface exists only where the capability was composed.
# The regexp variant defines it; the default Guest Binary, built without the
# capability, has no name for it at all.
class TestRegexpPresence < Minitest::Test
  include RegexpGuestHelper
  include GuestGuard

  DEFAULT_WASM = TestPaths.data("kobako.wasm")
  PROBE = "[Object.const_defined?(:Regexp), Object.const_defined?(:MatchData)]"

  # @behavior RX-215
  def test_only_a_regexp_capable_guest_defines_the_regexp_surface
    require_guest_binary!(DEFAULT_WASM, build: "bundle exec rake wasm:build")

    with_regexp = eval_regexp(PROBE)
    without = Kobako::Sandbox.new(wasm_path: DEFAULT_WASM).eval(PROBE).value

    assert_equal [[true, true], [false, false]], [with_regexp, without],
                 "only a regexp-capable Guest Binary must define Regexp and MatchData; the default must not"
  end
end
