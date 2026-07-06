# frozen_string_literal: true

require "test_helper"

# Differential parity — caps and usage (SPEC.md B-01, B-35): the
# per-invocation observables that mirror the configured caps must be
# present through both frontends.
class TestParityCapsUsage < Parity::Case
  # SPEC.md B-01 / B-35: a capped, successful invocation reports its
  # usage readout on both sides (values are timing-bound; parity pins
  # presence).
  def test_usage_present_after_success
    assert_parity Parity::Scenario.new(
      name: "usage-after-success", anchors: %w[B-01 B-35],
      options: { timeout_ms: 5000, memory_limit: 64 << 20 },
      invocations: [{ verb: "eval", source: "(1..100).reduce(:+)" }]
    )
  end

  # SPEC.md B-35: usage survives a trapped invocation — the readout
  # exists after a timeout on both sides.
  def test_usage_present_after_trap
    assert_parity Parity::Scenario.new(
      name: "usage-after-trap", anchors: %w[B-35],
      options: { timeout_ms: 300 },
      invocations: [{ verb: "eval", source: "loop { }" }]
    )
  end
end
