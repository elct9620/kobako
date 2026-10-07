# frozen_string_literal: true

# Shared setup for the JSON capability coverage under test/e2e/json/.
# kobako-json is opt-in, so its surface lives only in the json variant Guest
# Binary — these scenarios drive data/kobako+json.wasm and assert
# kobako-json's specified contract directly.
module JsonGuestHelper
  include GuestGuard

  JSON_WASM = TestPaths.data("kobako+json.wasm")

  def setup
    require_guest_binary!(JSON_WASM, build: "bundle exec rake wasm:build:json")
  end

  # Evaluate +code+ in a fresh Sandbox on the json guest. A fresh Sandbox
  # per scenario keeps capability state isolated between scenarios.
  def eval_json(code)
    Kobako::Sandbox.new(wasm_path: JSON_WASM).eval(code).value
  end

  # Assert +code+ reaches the host as a +Kobako::SandboxError+ carrying the
  # guest exception class +expected+ (the attribution of an uncaught guest
  # raise), and return the error so the caller can probe further.
  def assert_guest_raises(expected, code)
    err = assert_raises(Kobako::SandboxError) { eval_json(code) }
    assert_equal expected, err.klass,
                 "#{code.inspect} through the json guest must raise #{expected}"
    err
  end
end
