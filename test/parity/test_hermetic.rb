# frozen_string_literal: true

require "test_helper"

# Differential parity — profile posture (SPEC.md B-45, B-54): a
# requested isolation profile must build, declare, and floor-check
# identically on both frontends and reach the same observables.
#
# The frozen epoch and constant entropy themselves are not
# guest-observable — the pure default binary deliberately exposes no
# Time / entropy surface to compare, which is the very posture B-45
# pins, and the boot-state side of that determinism is pinned
# end-to-end in test/e2e/test_canonical_boot.rb. What a differential
# scenario pins here is the profile seam: the option flows through both
# frontends and neither diverges in accepting or enforcing it.
class TestParityHermetic < Parity::Case
  # SPEC.md B-45: an explicitly requested hermetic posture is honored
  # identically, and an eval under it observes the same result.
  def test_hermetic_profile_is_honored_identically
    assert_parity Parity::Scenario.new(
      name: "hermetic-profile", anchors: %w[B-45],
      options: { profile: "hermetic" },
      invocations: [{ verb: "eval", source: "1 + 1" }]
    )
  end

  # SPEC.md B-54: switching to the permissive posture floor-checks and
  # resolves the same way on both frontends — success or refusal, they
  # must agree.
  def test_permissive_profile_switch_is_identical
    assert_parity Parity::Scenario.new(
      name: "permissive-profile", anchors: %w[B-54],
      options: { profile: "permissive" },
      invocations: [{ verb: "eval", source: "1 + 1" }]
    )
  end
end
