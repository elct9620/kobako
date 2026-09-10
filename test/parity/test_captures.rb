# frozen_string_literal: true

require "test_helper"

# Differential parity — output captures: both frontends must expose the
# same captured bytes and the same truncation predicates after every
# invocation.
class TestParityCaptures < Parity::Case
  # @behavior S-034
  def test_streams_are_captured_separately
    assert_parity Parity::Scenario.new(
      name: "capture-streams",
      invocations: [
        { verb: "eval", source: 'puts "to out"; $stderr.puts "to err"; :done' }
      ]
    )
  end

  # A configured cap clips the stream and flips the truncation predicate
  # — identically on both sides.
  # @behavior S-035
  def test_truncation_at_the_cap
    assert_parity Parity::Scenario.new(
      name: "capture-truncation",
      options: { stdout_limit: 16, stderr_limit: 8 },
      invocations: [
        { verb: "eval", source: 'print "x" * 100; $stderr.print "y" * 100; :done' }
      ]
    )
  end
end
