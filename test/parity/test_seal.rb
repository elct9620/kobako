# frozen_string_literal: true

require "test_helper"

# Differential parity — seal-once registration (SPEC.md B-33): the
# first invocation seals the registration tables; a late registration
# is refused on both frontends (each in its own idiom, one observable
# status).
class TestParitySeal < Parity::Case
  # SPEC.md B-33: bind after the first invocation → sealed refusal.
  def test_late_registration_is_refused
    assert_parity Parity::Scenario.new(
      name: "late-registration", anchors: %w[B-33],
      invocations: [
        { verb: "eval", source: "1" },
        { verb: "late_bind", name: "LateService::KV" }
      ]
    )
  end
end
