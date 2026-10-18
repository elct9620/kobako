# frozen_string_literal: true

require "test_helper"

# Differential parity — caps and usage: the per-invocation observables
# that mirror the configured caps must be present through both frontends.
class TestParityCapsUsage < Parity::Case
  # A capped, successful invocation reports its usage readout on both
  # sides (values are timing-bound; parity pins presence). Usage survival
  # on the trap path is pinned by test_errors.rb's timeout scenario —
  # every assert_parity compares usage presence, so a second trap run here
  # would add no coverage.
  # @behavior S-063
  def test_usage_present_after_success
    assert_parity Parity::Scenario.new(
      name: "usage-after-success",
      options: { timeout_ms: 5000, memory_limit: 64 << 20 },
      invocations: [{ verb: "eval", source: "(1..100).reduce(:+)" }]
    )
  end

  OPEN_TWO = <<~RUBY
    first = File.open("/data/note.txt")
    begin
      File.open("/data/note.txt")
      [first.read, :both_opened]
    rescue Errno::EMFILE
      [first.read, :second_refused]
    end
  RUBY

  # The cap is one above the three standard descriptors and the Mount, so
  # the first open lands and the second is refused.
  # @behavior S-201
  def test_every_open_descriptor_counts_toward_the_open_file_cap
    assert_parity Parity::Scenario.new(
      name: "open-file-cap",
      options: { open_file_limit: 5 },
      mounts: [{ at: "/data", files: { "note.txt" => "hello" }, access: "read" }],
      invocations: [{ verb: "eval", source: OPEN_TWO }]
    )
  end
end
