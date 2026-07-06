# frozen_string_literal: true

require "test_helper"

# Differential parity — invocation isolation (SPEC.md B-02, B-03,
# B-49): both frontends must start every invocation from the canonical
# boot state, leaving no trace of the previous one.
class TestParityIsolation < Parity::Case
  # SPEC.md B-02 / B-03 / B-49: globals, constants, and reopened core
  # classes from one invocation are invisible to the next.
  def test_successive_invocations_are_isolated
    assert_parity Parity::Scenario.new(
      name: "invocation-isolation", anchors: %w[B-02 B-03 B-49],
      invocations: [
        { verb: "eval", source: "$leak = 41; LEAKED = 7; class String; def leaked?; true; end; end; :first" },
        { verb: "eval", source: '[$leak, defined?(LEAKED), "".respond_to?(:leaked?)]' }
      ]
    )
  end
end
