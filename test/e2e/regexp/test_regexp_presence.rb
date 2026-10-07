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

  # The capability keeps String's core [] / []= / index / split under
  # aliases it delegates to; each one is probed so a newly preserved method
  # cannot slip into the guest's surface.
  # @behavior RX-221
  def test_preserved_string_methods_are_not_callable_from_guest_code
    raised = %w[__kobako_aref(0) __kobako_aset(0,"x") __kobako_index("a") __kobako_split(",")].map do |call|
      guard_error(%("abc".#{call}), "NoMethodError")
    end

    assert_equal ["NoMethodError"] * 4, raised,
                 "a __kobako_ String alias called through eval must raise NoMethodError"
  end

  # @behavior RX-222
  # A global is reachable by whatever its name spells, so the witness asks
  # which names exist rather than reading one it expects to be absent.
  def test_compile_cache_is_not_reachable_as_a_global_variable
    assert_empty eval_regexp('"a" =~ /a/; global_variables.map(&:to_s).grep(/kobako|cache/)'),
                 "global_variables after a match through eval must name nothing after the capability or its cache"
  end
end
