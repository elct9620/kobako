# frozen_string_literal: true

require "test_helper"

# Differential parity — reflection denial: ambient reflection on a
# dispatch target must be refused as undefined on both frontends.
class TestParityReflection < Parity::Case
  ECHO_SERVICE = [
    { name: "MyService::KV", methods: { echo: { behavior: "echo" } } }
  ].freeze

  # `send` / `instance_eval` on a bound constant resolve to the undefined
  # fault, not to Kernel reflection.
  # @behavior T-131
  def test_reflection_on_target_is_undefined
    assert_parity Parity::Scenario.new(
      name: "reflection-denied",
      services: ECHO_SERVICE,
      invocations: [
        { verb: "eval", source: "MyService::KV.send(:echo, 1)" },
        { verb: "eval", source: 'MyService::KV.instance_eval("1")' }
      ]
    )
  end
end
