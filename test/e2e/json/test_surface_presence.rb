# frozen_string_literal: true

require "test_helper"

# E2E — the JSON surface exists only where the capability was composed.
# The json variant defines it; the default Guest Binary, built without the
# capability, has no name for it at all.
class TestJsonSurfacePresence < Minitest::Test
  include JsonGuestHelper
  include GuestGuard

  DEFAULT_WASM = TestPaths.data("kobako.wasm")
  PROBE = "Object.const_defined?(:JSON)"

  # @behavior JS-058
  def test_only_a_json_capable_guest_defines_the_json_surface
    require_guest_binary!(DEFAULT_WASM, build: "bundle exec rake wasm:build")

    with_json = eval_json(PROBE)
    without = Kobako::Sandbox.new(wasm_path: DEFAULT_WASM).eval(PROBE).value

    assert_equal [true, false], [with_json, without],
                 "only a JSON-capable Guest Binary must define JSON; the default Guest Binary must not"
  end
end
