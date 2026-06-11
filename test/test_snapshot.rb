# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of +Kobako::Snapshot+ — the per-invocation
# observable bundle +Runtime#eval+ / +#run+ returns. Drives +Runtime+
# directly (bypassing +Sandbox+) against the real +data/kobako.wasm+
# so the contract being pinned is "what the ext hands back", not the
# Sandbox-side decomposition.
#
# Sandbox-level usage of the same fields is covered through
# +test/test_sandbox_usage.rb+ and the +test/e2e/+ journeys; this
# file deliberately stays at the Runtime seam so a regression in the
# Snapshot magnus wrap or the Ruby helper layer surfaces here, not via
# indirect Sandbox assertions.
class TestSnapshot < Minitest::Test
  KOBAKO_WASM = File.expand_path("../data/kobako.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Snapshot)
    skip "guest wasm not built (run `bundle exec rake wasm:build`)" unless File.exist?(KOBAKO_WASM)
  end

  # Every raw reader returns the documented Ruby type. The ext side
  # encodes the slot data into seven specific shapes (binary String for
  # the byte fields, bool for the truncation flags, Float for wall_time,
  # Integer for memory_peak) — pin them so a magnus binding change cannot
  # silently shift the type.
  def test_eval_returns_snapshot_with_documented_raw_field_types
    snapshot = drive_eval("42")

    assert_instance_of Kobako::Snapshot, snapshot
    assert_kind_of String, snapshot.return_bytes
    assert_kind_of String, snapshot.stdout_bytes
    assert_kind_of String, snapshot.stderr_bytes
    assert_includes [true, false], snapshot.stdout_truncated
    assert_includes [true, false], snapshot.stderr_truncated
    assert_kind_of Float,   snapshot.wall_time
    assert_kind_of Integer, snapshot.memory_peak
  end

  # +#stdout+ packs the +stdout_bytes+ / +stdout_truncated+ raw pair into
  # a +Kobako::Capture+ value object. Pinning the raw-to-helper mapping
  # catches a regression where the helper accidentally reads the
  # stderr-side fields — the kind of swap a magnus reader rename could
  # introduce silently. +Capture.new+ force-encodes the bytes, so
  # compare via +.b+ to isolate the assertion from UTF-8 / ASCII-8BIT
  # drift.
  def test_stdout_helper_packs_stdout_pair_into_capture
    snapshot = drive_eval("42")

    assert_instance_of Kobako::Capture, snapshot.stdout
    assert_equal snapshot.stdout_bytes.b, snapshot.stdout.bytes.b
    assert_equal snapshot.stdout_truncated, snapshot.stdout.truncated?
  end

  # +#stderr+ mirrors +#stdout+ — same Capture packing, same anti-swap
  # invariant against the stdout-side raw fields.
  def test_stderr_helper_packs_stderr_pair_into_capture
    snapshot = drive_eval("42")

    assert_instance_of Kobako::Capture, snapshot.stderr
    assert_equal snapshot.stderr_bytes.b, snapshot.stderr.bytes.b
    assert_equal snapshot.stderr_truncated, snapshot.stderr.truncated?
  end

  # +#usage+ packs the +wall_time+ / +memory_peak+ raw pair into a
  # +Kobako::Usage+ value object. Same anti-swap invariant — the
  # destructure-to-kwargs handoff must keep field order correct.
  def test_usage_helper_packs_wall_time_and_memory_peak
    snapshot = drive_eval("42")

    assert_instance_of Kobako::Usage, snapshot.usage
    assert_equal snapshot.wall_time, snapshot.usage.wall_time
    assert_equal snapshot.memory_peak, snapshot.usage.memory_peak
  end

  # End-to-end stdout capture through the Snapshot pipeline: guest writes
  # via +puts+, host reads through +snapshot.stdout+. This is the path
  # the Sandbox-facing +#stdout+ reader ultimately depends on, so the
  # assertion proves the capture chain from WASI pipe → Invocation slot
  # → Snapshot.stdout_bytes → Capture#bytes is end-to-end live.
  def test_snapshot_stdout_reflects_guest_writes
    snapshot = drive_eval("puts 'hello-from-snapshot-test'")

    assert_includes snapshot.stdout.bytes, "hello-from-snapshot-test"
    refute snapshot.stdout.truncated?
  end

  # Wall-clock and memory accounting are populated on every successful
  # invocation (B-35). Lock the non-negative invariants so a measurement
  # bug that produces NaN / negative integers is caught.
  def test_snapshot_usage_carries_non_negative_wall_time_and_memory_peak
    snapshot = drive_eval("42")

    assert_operator snapshot.wall_time, :>=, 0.0
    assert_operator snapshot.memory_peak, :>=, 0
  end

  private

  # Minimal Runtime driver that mirrors +Sandbox#eval+'s wiring without
  # the Sandbox wrapper. Builds an empty Catalog::Namespaces / Snippet table
  # so the encoded preamble + encoded snippets are both wire-valid, registers
  # a guard Proc on +on_dispatch=+ (no Service callbacks expected from
  # the simple eval sources used by these tests), and returns the raw
  # Snapshot the ext produces.
  def drive_eval(code)
    handler = Kobako::Catalog::Handles.new
    services = Kobako::Catalog::Namespaces.new(handler: handler)
    snippets = Kobako::Catalog::Snippets.new

    runtime = Kobako::Runtime.from_path(KOBAKO_WASM, nil, nil, nil, nil)
    runtime.on_dispatch = ->(_) { raise "unexpected dispatch in eval-only snapshot test" }

    runtime.eval(services.encode, code.b, snippets.encode)
  end
end
