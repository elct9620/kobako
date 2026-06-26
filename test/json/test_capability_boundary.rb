# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the JSON capability-reference boundary through the real
# json guest (SPEC.md B-53). parse cannot fabricate a host capability, and
# generate refuses one rather than dispatching to the host.
class TestJsonCapabilityBoundary < Minitest::Test
  include JsonGuestHelper

  # A host object a bound Service returns, reaching the guest as a
  # Kobako::Handle.
  class Greeter
    def greet = "hi"
  end

  # Guest code that smuggles a Handle inside an opt-in object's as_json
  # result, exercising the depth-first refusal.
  NESTED_HANDLE = <<~RUBY
    h = Source::Get.call
    o = Object.new
    o.define_singleton_method(:as_json) { { handle: h } }
    JSON.generate(o)
  RUBY

  # B-53 (inbound): no JSON syntax decodes to a Handle — a parsed object is
  # an ordinary Hash exposing no host capability, so a guest cannot forge
  # one through parse.
  def test_b53_parse_cannot_forge_a_handle
    assert_equal "Hash", eval_json('JSON.parse(%q({"greet":"x"})).class.to_s'),
                 "JSON.parse through the json guest must yield an ordinary Hash, never a host capability"
    assert_equal false, eval_json('JSON.parse(%q({"greet":"x"})).respond_to?(:greet)'),
                 "a value JSON.parse produced must not answer to a host Service method — it is no capability"
  end

  # B-53 (outbound): a bare Handle reaches the generate converter and is
  # refused with GeneratorError. The error is the witness that the
  # Object-rooted as_json default fired locally — a host round-trip would
  # surface a different outcome, not a JSON::GeneratorError.
  def test_b53_generate_refuses_a_bare_handle
    sandbox = Kobako::Sandbox.new(wasm_path: JsonGuestHelper::JSON_WASM)
    sandbox.define(:Source).bind(:Get, -> { Greeter.new })

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("h = Source::Get.call; JSON.generate(h)") }

    assert_equal "JSON::GeneratorError", err.klass,
                 "JSON.generate of a Handle through the json guest must raise GeneratorError, not dispatch to the host"
  end

  # B-53 (outbound): a Handle smuggled inside an opt-in object's as_json
  # result is still refused — the depth-first walk reaches it and the same
  # boundary fires.
  def test_b53_generate_refuses_a_handle_nested_in_an_as_json_result
    sandbox = Kobako::Sandbox.new(wasm_path: JsonGuestHelper::JSON_WASM)
    sandbox.define(:Source).bind(:Get, -> { Greeter.new })

    err = assert_raises(Kobako::SandboxError) { sandbox.eval(NESTED_HANDLE) }

    assert_equal "JSON::GeneratorError", err.klass,
                 "JSON.generate must refuse a Handle nested in an as_json result, not dispatch it to the host"
  end
end
