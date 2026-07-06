# frozen_string_literal: true

require "test_helper"

# Differential parity — output captures (SPEC.md B-04): both frontends
# must expose the same captured bytes and the same truncation
# predicates after every invocation.
class TestParityCaptures < Parity::Case
  # SPEC.md B-04: stdout and stderr arrive as separate byte streams.
  def test_streams_are_captured_separately
    assert_parity Parity::Scenario.new(
      name: "capture-streams", anchors: %w[B-04],
      invocations: [
        { verb: "eval", source: 'puts "to out"; $stderr.puts "to err"; :done' }
      ]
    )
  end

  # SPEC.md B-04: a configured cap clips the stream and flips the
  # truncation predicate — identically on both sides.
  def test_truncation_at_the_cap
    assert_parity Parity::Scenario.new(
      name: "capture-truncation", anchors: %w[B-04],
      options: { stdout_limit: 16, stderr_limit: 8 },
      invocations: [
        { verb: "eval", source: 'print "x" * 100; $stderr.print "y" * 100; :done' }
      ]
    )
  end
end
