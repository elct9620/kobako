# frozen_string_literal: true

require "test_helper"

# Differential parity — reflection denial (SPEC.md B-42, B-43, E-43,
# E-44): ambient reflection on a dispatch target must be refused as
# undefined on both frontends; reflective gadgets must never become
# Handles.
class TestParityReflection < Parity::Case
  ECHO_SERVICE = [
    { namespace: "MyService", member: "KV", methods: { echo: { behavior: "echo" } } }
  ].freeze

  # SPEC.md B-42 / E-43: `send` / `instance_eval` on a bound Member
  # resolve to the undefined fault, not to Kernel reflection.
  def test_reflection_on_target_is_undefined
    assert_parity Parity::Scenario.new(
      name: "reflection-denied", anchors: %w[B-42 E-43],
      services: ECHO_SERVICE,
      invocations: [
        { verb: "eval", source: "MyService::KV.send(:echo, 1)" },
        { verb: "eval", source: 'MyService::KV.instance_eval("1")' }
      ]
    )
  end

  # SPEC.md B-43 / E-44: a Service returning a reflective gadget
  # (Method / Binding) is refused rather than wrapped into a Handle.
  # Reflective gadgets are Ruby surface with no Rust counterpart, so no
  # stub behavior can express a gadget return from the SDK; the Ruby
  # refusal is pinned by test/transport/test_dispatcher_gadget_return.rb
  # and test/catalog/test_handles.rb.
  def test_gadget_return_pending
    skip "B-43 E-44 gadgets have no Rust counterpart; the refusal is pinned on the Ruby side"
  end
end
