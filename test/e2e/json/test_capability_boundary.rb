# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the JSON capability-reference boundary through the real
# json guest. parse cannot fabricate a host capability, and
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

  # Inbound: no JSON syntax decodes to a Handle — a parsed object is an
  # ordinary Hash exposing no host capability.
  # @behavior JS-041
  def test_parse_cannot_forge_a_handle
    assert_equal "Hash", eval_json('JSON.parse(%q({"greet":"x"})).class.to_s'),
                 "JSON.parse through the json guest must yield an ordinary Hash, never a host capability"
    assert_equal false, eval_json('JSON.parse(%q({"greet":"x"})).respond_to?(:greet)'),
                 "a value JSON.parse produced must not answer to a host Service method — it is no capability"
  end

  # GeneratorError is the witness that the Object-rooted as_json default
  # fired locally — a host round-trip would surface a different outcome.
  # @behavior JS-042
  def test_generate_refuses_a_bare_handle
    sandbox = Kobako::Sandbox.new(wasm_path: JsonGuestHelper::JSON_WASM)
    sandbox.bind("Source::Get", -> { Greeter.new })

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("h = Source::Get.call; JSON.generate(h)") }

    assert_equal "JSON::GeneratorError", err.klass,
                 "JSON.generate of a Handle through the json guest must raise GeneratorError, not dispatch to the host"
  end

  # The depth-first walk reaches a Handle smuggled inside an opt-in object's
  # as_json result, so the same boundary fires.
  # @behavior JS-043
  def test_generate_refuses_a_handle_nested_in_an_as_json_result
    sandbox = Kobako::Sandbox.new(wasm_path: JsonGuestHelper::JSON_WASM)
    sandbox.bind("Source::Get", -> { Greeter.new })

    err = assert_raises(Kobako::SandboxError) { sandbox.eval(NESTED_HANDLE) }

    assert_equal "JSON::GeneratorError", err.klass,
                 "JSON.generate must refuse a Handle nested in an as_json result, not dispatch it to the host"
  end

  # The key boundary is distinct from the value boundary: a host-dispatching
  # to_s would stringify the Handle and let generate succeed.
  # @behavior JS-044
  def test_generate_refuses_a_handle_used_as_a_hash_key
    sandbox = Kobako::Sandbox.new(wasm_path: JsonGuestHelper::JSON_WASM)
    sandbox.bind("Source::Get", -> { Greeter.new })

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("h = Source::Get.call; JSON.generate({ h => 1 })") }

    assert_equal "JSON::GeneratorError", err.klass,
                 "a Handle Hash key through JSON.generate must be refused at the key boundary, not host-dispatched"
  end

  # The as_json hook is itself no host-penetration path: were the
  # Object-rooted default unshadowed, the call would fall to the Handle's
  # method_missing and dispatch as_json to the host.
  # @behavior JS-045
  def test_calling_as_json_on_a_handle_raises_locally_without_host_dispatch
    sandbox = Kobako::Sandbox.new(wasm_path: JsonGuestHelper::JSON_WASM)
    sandbox.bind("Source::Get", -> { Greeter.new })

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("h = Source::Get.call; h.as_json") }

    assert_equal "JSON::GeneratorError", err.klass,
                 "as_json on a Handle must raise the Object-rooted default locally, not dispatch to the host"
  end
end
