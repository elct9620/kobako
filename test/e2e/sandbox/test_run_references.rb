# frozen_string_literal: true

require "test_helper"

# E2E — a reference #run wrapped from an entrypoint's argument, followed
# back out, through real mruby. The wrap is the host's own, so the same
# object comes back wherever the reference goes next: into a Service as an
# argument, or out to the Host App as the entrypoint's answer.
class TestE2ERunReferences < Minitest::Test
  include E2eGuestHelper

  # A request body the Host App wrote; it has no wire representation.
  class Body
    def read = "payload"
  end

  # @behavior T-246
  def test_a_wrapped_argument_passed_to_a_service_arrives_as_the_original_object
    body = Body.new
    received = []
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store::Keep", ->(item) { received << item })
    sandbox.preload(code: "Forward = ->(item) { Store::Keep.call(item); nil }", name: :Forward)

    sandbox.run(:Forward, body)

    assert_same body, received.first,
                "a reference wrapped from a #run argument and passed to a Service must arrive as " \
                "the original host object"
  end

  # @behavior T-247
  def test_an_entrypoint_returning_a_service_reference_hands_back_the_original_object
    body = Body.new
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store::Fetch", -> { body })
    sandbox.preload(code: "Fetched = ->(*) { Store::Fetch.call }", name: :Fetched)

    assert_same body, sandbox.run(:Fetched).value,
                "an entrypoint returning a reference through #run must hand the Host App the " \
                "original host object"
  end

  # @behavior T-247
  def test_an_entrypoint_returning_its_wrapped_argument_hands_back_the_original_object
    body = Body.new
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Given = ->(item) { item }", name: :Given)

    assert_same body, sandbox.run(:Given, body).value,
                "an entrypoint returning a reference wrapped from its own #run arguments must " \
                "hand the Host App the original host object"
  end
end
