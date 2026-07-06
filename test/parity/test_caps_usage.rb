# frozen_string_literal: true

require "test_helper"

# Differential parity — caps and usage (SPEC.md B-01, B-35): the
# per-invocation observables that mirror the configured caps must be
# present through both frontends.
class TestParityCapsUsage < Parity::Case
  # SPEC.md B-01 / B-35: a capped, successful invocation reports its
  # usage readout on both sides (values are timing-bound; parity pins
  # presence). Usage survival on the trap path is pinned by
  # test_errors.rb's timeout scenario — every assert_parity compares
  # usage presence, so a second trap run here would add no coverage.
  def test_usage_present_after_success
    assert_parity Parity::Scenario.new(
      name: "usage-after-success", anchors: %w[B-01 B-35],
      options: { timeout_ms: 5000, memory_limit: 64 << 20 },
      invocations: [{ verb: "eval", source: "(1..100).reduce(:+)" }]
    )
  end
end
