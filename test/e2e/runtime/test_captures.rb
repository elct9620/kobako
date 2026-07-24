# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of the +Kobako::Runtime::Snapshot+ each invocation
# entry point hands back — its outcome bytes, the two output captures, and
# the usage readout. Drives +Runtime+ directly (bypassing +Sandbox+)
# against the real +data/kobako.wasm+ so the contract being pinned is "what
# the ext hands back", not the Sandbox-side decomposition.
#
# Sandbox-level consumption of the same Snapshot is covered through
# +test/sandbox/+ and the +test/e2e/+ journeys (including the B-04
# captures-on-trap cases in +test/e2e/test_caps.rb+); this file
# deliberately stays at the Runtime seam so a regression in the magnus
# binding surfaces here, not via indirect Sandbox assertions.
class TestRuntimeCaptures < Minitest::Test
  KOBAKO_WASM = TestPaths.data("kobako.wasm")

  def setup
    # `rake test` builds both prerequisites, so under CI a missing one is
    # a broken pipeline, never a skip — mirroring E2eGuestHelper.
    unless defined?(Kobako::Runtime)
      flunk "native ext not compiled under CI" if ENV["CI"]
      skip "native ext not compiled (run `bundle exec rake compile`)"
    end
    return if File.exist?(KOBAKO_WASM)

    flunk "data/kobako.wasm missing under CI" if ENV["CI"]
    skip "guest wasm not built (run `bundle exec rake wasm:build`)"
  end

  # The ext encodes the Snapshot fields into specific Ruby shapes (binary
  # String for the byte fields, bool for the truncation flags, Float /
  # Integer for usage) — pin them so a magnus binding change cannot silently
  # shift a type past RBS, which does not verify what a C extension actually
  # returns.
  def test_snapshot_exposes_documented_raw_types_on_a_completed_run
    snapshot = drive_eval("42")

    refute_predicate snapshot, :trapped?
    assert_kind_of String, snapshot.outcome
    assert_kind_of String, snapshot.stdout
    assert_kind_of String, snapshot.stderr
    assert_includes [true, false], snapshot.stdout_truncated?
    assert_includes [true, false], snapshot.stderr_truncated?
    assert_kind_of Float, snapshot.wall_time
    assert_kind_of Integer, snapshot.memory_peak
  end

  # The two capture channels are distinct readers; a reader swap in the ext
  # would silently cross the channels. Writing distinct content to each
  # channel in one run pins stdout to #stdout and stderr to #stderr.
  def test_snapshot_keeps_stdout_and_stderr_channels_apart
    snapshot = drive_eval('$stdout.puts "to-out"; $stderr.puts "to-err"; 1')

    assert_equal "to-out\n", snapshot.stdout, "#stdout must carry the stdout channel"
    assert_equal "to-err\n", snapshot.stderr, "#stderr must carry the stderr channel"
  end

  # A run that writes nothing yields empty captures with the flags down, so
  # the Sandbox's Capture wrapping never leaks a previous process state or
  # nil into the captures it exposes.
  def test_snapshot_of_a_silent_run_has_empty_captures
    snapshot = drive_eval("1 + 1")

    assert_equal "", snapshot.stdout
    assert_equal "", snapshot.stderr
    refute_predicate snapshot, :stdout_truncated?
    refute_predicate snapshot, :stderr_truncated?
  end

  private

  # Minimal Runtime driver that mirrors +Sandbox#eval+'s wiring without the
  # Sandbox wrapper. Builds an empty Catalog::Services / Snippet table so the
  # encoded preamble + encoded snippets are both wire-valid, hands the
  # per-call dispatch handler a guard Proc (no Service callbacks expected
  # from the simple eval sources used here), and returns the
  # +Kobako::Runtime::Snapshot+.
  def drive_eval(code)
    services = Kobako::Catalog::Services.new
    snippets = Kobako::Catalog::Snippets.new
    runtime = Kobako::Runtime.from_path(KOBAKO_WASM, nil, nil, nil, nil, :hermetic)
    dispatch = ->(_, _) { raise "unexpected dispatch in eval-only captures test" }

    runtime.eval(dispatch, services.encode, code.b, snippets.encode)
  end
end
