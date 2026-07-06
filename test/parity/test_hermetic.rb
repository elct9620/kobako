# frozen_string_literal: true

require "test_helper"

# Differential parity — hermetic determinism (SPEC.md B-45): under the
# hermetic profile both frontends must observe the same frozen epoch
# and constant entropy.
class TestParityHermetic < Parity::Case
  # SPEC.md B-45: needs an ambient-observable guest path (the pure
  # default binary exposes no Time / entropy surface to compare).
  def test_hermetic_determinism_pending
    skip "pending an ambient-observable guest path (B-45)"
  end
end
