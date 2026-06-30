# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the stdout / stderr capture channels through real mruby
# (SPEC.md B-04): routing, truncation caps, per-run reset (B-03), and
# $stdout reassignment semantics. Kernel-level write primitives live in
# test_io_kernel.rb; the IO write byte paths in test_io_write.rb.
class TestE2EIoStreams < Minitest::Test
  include E2eGuestHelper

  # mruby's +puts+ on a capped channel may raise +IOError+ once the
  # WASI write is rejected. The rescue swallows that script-level
  # failure so these tests pin only the host-observable contract
  # (clipped bytes + predicate); whether the failure surfaces as a
  # raised error or a silently-short write is intentionally not pinned.
  OVERFLOW_SCRIPT = 'begin; puts "long enough to overflow the 5-byte cap"; rescue StandardError; end; 1'

  # Symmetric overflow script for the stderr channel — uses +$stderr.puts+
  # directly because +Kernel#warn+ would route through the same global
  # but adds nothing to the truncation observation.
  OVERFLOW_STDERR_SCRIPT =
    'begin; $stderr.puts "long enough to overflow the 5-byte cap"; rescue StandardError; end; 1'

  # SPEC.md B-04: output past +stdout_limit+ is clipped at the cap
  # boundary, +#stdout+ carries no truncation sentinel, and
  # +#stdout_truncated?+ flips to +true+. The cap is enforced inside the
  # WASI pipe — +#run+ still returns the script's last expression.
  def test_stdout_truncation_flag_when_output_exceeds_cap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, stdout_limit: 5)
    result = sandbox.eval(OVERFLOW_SCRIPT)

    assert_equal 1, result
    # The retained bytes are the real first 5 of the overflowing line
    # ("long enough …" → "long "), clipped at the boundary — not a bounded
    # length alone, which a regression dropping or reordering content could
    # still satisfy, and not a sentinel.
    assert_equal "long ", sandbox.stdout,
                 "an overflowing stdout write must retain exactly its first stdout_limit bytes, no sentinel"
    refute_includes sandbox.stdout, "[truncated]"
    assert sandbox.stdout_truncated?
  end

  # SPEC.md B-03: truncation predicates reset together with the capture
  # buffers at the start of the next +#run+.
  def test_stdout_truncated_predicate_resets_between_runs
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, stdout_limit: 5)
    sandbox.eval(OVERFLOW_SCRIPT)
    assert sandbox.stdout_truncated?, "setup: first run must overflow the cap"

    sandbox.eval("nil")
    refute sandbox.stdout_truncated?, "B-03: stdout_truncated? must reset on the next run"
    assert_equal "", sandbox.stdout
  end

  # SPEC.md B-04: $stderr writes land in Sandbox#stderr, not Sandbox#stdout.
  # Covers the guest-side fd 2 path enabled by the kobako-io ::IO gem.
  # The equality assertion rejects install-time noise (e.g. mruby's +mrb_warn+
  # for a NULL super class) leaking onto fd 2 — the guest's own +$stderr.puts+
  # output is the only thing the channel may carry on this run.
  def test_stderr_puts_routes_to_stderr_channel
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('$stderr.puts "diagnostic"; 1')

    assert_equal "diagnostic\n", sandbox.stderr,
                 "B-04: $stderr.puts must reach Sandbox#stderr exclusively"
    assert_empty sandbox.stdout,
                 "B-04: stderr writes must not bleed into Sandbox#stdout"
  end

  # SPEC.md B-04: Kernel#warn delegates through $stderr per the kobako-io
  # Kernel delegators,
  # so warned bytes show up on Sandbox#stderr like any other stderr write.
  # The equality assertion also rejects install-time noise (e.g. mruby's
  # +mrb_warn+ for a NULL super class) leaking onto fd 2 — the guest's own
  # +warn+ output is the only thing the channel may carry on this run.
  def test_warn_routes_to_stderr_channel
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('warn "caution"; 1')

    assert_equal "caution\n", sandbox.stderr,
                 "Kernel#warn must route only the guest message through $stderr"
    assert_empty sandbox.stdout,
                 "Kernel#warn must not bleed into stdout"
  end

  # Reassigning $stdout = $stderr at script time must redirect subsequent
  # Kernel#puts output to the stderr capture channel. This is the whole
  # reason Kernel delegators route through the assignable global instead
  # of hard-coded fd 1, and verifies that the kobako-io Kernel delegators
  # honor the late binding.
  def test_redirecting_stdout_to_stderr_routes_subsequent_puts
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('$stdout = $stderr; puts "redirected"; 1')

    assert_includes sandbox.stderr, "redirected",
                    "Kernel#puts after `$stdout = $stderr` must follow the reassignment"
    refute_includes sandbox.stdout, "redirected",
                    "Original stdout channel must stay empty after redirection"
  end

  # Reassigning $stdout inside a #run must not bleed into the next
  # #run — each invocation rebuilds the mruby state and reinstalls
  # the globals, so a subsequent puts always lands on the host's
  # stdout channel. Pins this per-run-reset invariant explicitly
  # because the redirection test alone would not catch a regression
  # that made the reassignment persistent.
  def test_stdout_assignment_does_not_persist_across_runs
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    sandbox.eval('$stdout = $stderr; puts "first"; 1')
    assert_includes sandbox.stderr, "first", "setup: first run must redirect"

    sandbox.eval('puts "second"; 2')
    assert_includes sandbox.stdout, "second",
                    "second run must restore $stdout to the stdout channel"
    refute_includes sandbox.stderr, "second",
                    "second run must not inherit the previous run's $stdout reassignment"
  end

  # Symmetric to test_stdout_truncation_flag_when_output_exceeds_cap.
  # Cap is enforced inside the WASI pipe on fd 2; #stderr never contains
  # truncation sentinels.
  def test_stderr_truncation_flag_when_output_exceeds_cap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, stderr_limit: 5)
    result = sandbox.eval(OVERFLOW_STDERR_SCRIPT)

    assert_equal 1, result
    assert_equal "long ", sandbox.stderr,
                 "an overflowing stderr write must retain exactly its first stderr_limit bytes, no sentinel"
    refute_includes sandbox.stderr, "[truncated]"
    assert sandbox.stderr_truncated?
  end

  # L5 / B-01: an explicit nil output cap leaves the channel uncapped, so
  # output far past the 1 MiB default is captured in full and the predicate
  # stays false. Proves the ext's uncapped (None → usize::MAX) capture path
  # is reachable from the Sandbox once nil disables the bound. memory_limit
  # is also lifted so the 2 MiB the guest builds is not stopped by the
  # memory cap before it can reach the capture pipe.
  def test_nil_stdout_limit_captures_output_past_the_default_cap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, stdout_limit: nil, memory_limit: nil)
    # Written as four sub-MiB chunks: a single 2 MiB mruby String would trip
    # the interpreter's own 1 MiB string cap before reaching the pipe.
    chunk = 512 * 1024
    count = 4
    script = %(#{count}.times { print "x" * #{chunk} }; 1)

    sandbox.eval(script)

    assert_equal chunk * count, sandbox.stdout.bytesize,
                 "stdout_limit: nil must capture the full output past the default 1 MiB cap"
    refute sandbox.stdout_truncated?,
           "an uncapped stdout channel must never report truncation"
  end

  # SPEC.md B-04: stdout buffer is per-run; second #run does not see first run's output.
  def test_stdout_is_per_run_b04
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    sandbox.eval('puts "first"; 1')
    assert_includes sandbox.stdout, "first"

    sandbox.eval('puts "second"; 2')
    refute_includes sandbox.stdout, "first",
                    "B-04: stdout must reset between runs (SPEC.md B-04 L264-270)"
    assert_includes sandbox.stdout, "second"
  end
end
