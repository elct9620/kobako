# frozen_string_literal: true

# Shared setup for the JSON capability coverage under test/json/ (SPEC.md
# B-52 / B-53, docs/json.md JS-01..09). kobako-json is opt-in, so its
# surface lives only in the json variant Guest Binary — these scenarios
# drive data/kobako+json.wasm and assert the JS-xx contract directly.
module JsonGuestHelper
  JSON_WASM = File.expand_path("../../data/kobako+json.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    return if File.exist?(JSON_WASM)

    skip "data/kobako+json.wasm missing — run `bundle exec rake wasm:build:json`"
  end

  # Evaluate +code+ in a fresh Sandbox on the json guest. A fresh Sandbox
  # per scenario keeps capability state isolated between scenarios.
  def eval_json(code)
    Kobako::Sandbox.new(wasm_path: JSON_WASM).eval(code)
  end

  # Assert +code+ reaches the host as a +Kobako::SandboxError+ carrying the
  # guest exception class +expected+ (E-04 attribution of an uncaught
  # guest raise), and return the error so the caller can probe further.
  def assert_guest_raises(expected, code)
    err = assert_raises(Kobako::SandboxError) { eval_json(code) }
    assert_equal expected, err.klass,
                 "#{code.inspect} through the json guest must raise #{expected}"
    err
  end
end
