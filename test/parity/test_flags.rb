# frozen_string_literal: true

require "test_helper"

# Differential parity — flags the bundled Guest Binary does not recognise:
# a critical one fails the invocation under the same status on both
# frontends, and an advisory one leaves the run as it would be without it.
class TestParityFlags < Parity::Case
  # @behavior S-183
  def test_a_critical_flag_the_guest_does_not_recognise_fails_alike
    assert_parity Parity::Scenario.new(
      name: "critical-unrecognised-flag",
      flags: [{ name: "no_such_flag", value: "", advisory: false }],
      invocations: [{ verb: "eval", source: "puts 'ran'; 1" }]
    )
  end

  # @behavior S-184
  def test_an_advisory_flag_the_guest_does_not_recognise_is_skipped_alike
    assert_parity Parity::Scenario.new(
      name: "advisory-unrecognised-flag",
      flags: [{ name: "no_such_flag", value: "8", advisory: true }],
      invocations: [{ verb: "eval", source: "puts 'ran'; 1" }]
    )
  end
end
