# frozen_string_literal: true

require "test_helper"

# Differential parity — hermetic determinism (SPEC.md B-45): under the
# hermetic profile both frontends must observe the same frozen epoch
# and constant entropy.
class TestParityHermetic < Parity::Case
  # SPEC.md B-45: needs an ambient-observable guest path — the pure
  # default binary deliberately exposes no Time / entropy surface to
  # compare, which is the very posture B-45 pins. The boot-state side
  # of that determinism is pinned end-to-end in
  # test/e2e/test_canonical_boot.rb.
  def test_hermetic_determinism_pending
    skip "pending an ambient-observable guest path (B-45)"
  end
end
