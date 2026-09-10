# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the per-invocation resource caps through real mruby: the
# wall-clock timeout and linear-memory delta traps, their TrapError
# taxonomy, per-invocation re-anchoring, and Sandbox reusability after a
# trap.
class TestE2ECaps < Minitest::Test
  include E2eGuestHelper

  # The cap raises `Kobako::TimeoutError`, a `Kobako::TrapError` subclass,
  # so callers that only care about the unrecoverable outcome can rescue
  # the base class.
  # @behavior OC-032 S-149
  def test_timeout_cap_traps_infinite_loop
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    started = Time.now
    err = assert_raises(Kobako::TimeoutError) { sandbox.eval("loop { }") }
    elapsed = Time.now - started

    assert_kind_of Kobako::TrapError, err,
                   "TimeoutError must be a TrapError subclass"
    assert_operator elapsed, :<, 2.0,
                    "timeout must fire within the configured budget (epoch ticker latency aside)"
    assert_match(/timeout|wall-clock/i, err.message)
  end

  # The cap measures only the growth attributable to this invocation — the
  # mruby image's initial allocation and prior invocations' watermark sit
  # outside the budget — so a runaway script still surfaces as a trap.
  # @behavior OC-033
  def test_memory_limit_cap_traps_runaway_allocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 2 << 20)

    err = assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('a = []; 200.times { a << ("x" * 100_000) }; nil')
    end

    assert_kind_of Kobako::TrapError, err,
                   "MemoryLimitError must be a TrapError subclass"
    assert_match(/memory_limit/, err.message)
  end

  # The cap re-anchors at each invocation's entry, so back-to-back scripts
  # whose combined high-water mark exceeds it still each run within budget.
  # @behavior S-008
  def test_memory_limit_resets_per_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 1 << 20)

    assert_equal 200_000, sandbox.eval('a = "x" * 200_000; a.bytesize').value
    assert_equal 200_000, sandbox.eval('a = "x" * 200_000; a.bytesize').value
  end

  # Pins that the cap is wired through the real guest at the default 1 MiB
  # budget, not some far larger figure; the exact-threshold bisection lives
  # in the cargo `KobakoLimiter` unit tests.
  # @behavior OC-033
  def test_memory_limit_traps_single_invocation_past_default_cap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 1 << 20)

    err = assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('a = []; 100.times { a << ("x" * 50_000) }; nil')
    end

    assert_match(/memory_limit/, err.message)
  end

  # The cap window `Runtime#eval` opens is closed whether the guest returns
  # or traps, so the next invocation never inherits the trapped run's armed
  # deadline. Reuse after success and after a guest raise are pinned
  # elsewhere; this case closes the gap of reuse after a host *trap*.
  # @behavior S-010
  def test_sandbox_reusable_after_timeout_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    assert_raises(Kobako::TimeoutError) { sandbox.eval("loop { }") }

    assert_equal 3, sandbox.eval("1 + 2").value,
                 "a Sandbox must stay usable after a TimeoutError — the next " \
                 "eval must run under a fresh cap window, not re-trap on the old one"
  end

  # The MemoryLimitError counterpart of the timeout-recovery case above: the
  # limiter re-anchors its baseline per invocation rather than staying armed
  # at the trapped run's watermark.
  # @behavior S-011
  def test_sandbox_reusable_after_memory_limit_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 1 << 20)

    assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('a = []; 100.times { a << ("x" * 50_000) }; nil')
    end

    assert_equal 200_000, sandbox.eval('a = "x" * 200_000; a.bytesize').value,
                 "a Sandbox must stay usable after a MemoryLimitError — the next " \
                 "within-budget eval must succeed under a re-anchored cap window"
  end

  # The guest writes both channels in the one run because a mid-invocation
  # kill is the one moment the two capture pipes could plausibly diverge;
  # this case asserts the stdout half, the case below the stderr half.
  # @behavior S-036 S-134
  def test_partial_stdout_readable_after_timeout_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    error = assert_raises(Kobako::TimeoutError) do
      sandbox.eval('$stdout.puts "out before trap"; $stderr.puts "err before trap"; loop { }')
    end

    assert_equal "out before trap\n", error.execution.stdout,
                 "stdout written before a TimeoutError must stay readable " \
                 "after the rescue"
    refute_predicate error.execution, :stdout_truncated?,
                     "a trap is not a cap overflow — the truncation " \
                     "predicate must stay false"
  end

  # The stderr half of the two-channel trap run above.
  # @behavior S-037
  def test_partial_stderr_readable_after_timeout_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    error = assert_raises(Kobako::TimeoutError) do
      sandbox.eval('$stdout.puts "out before trap"; $stderr.puts "err before trap"; loop { }')
    end

    assert_equal "err before trap\n", error.execution.stderr,
                 "stderr written before a TimeoutError must stay readable " \
                 "after the rescue"
  end

  # The rescued overflow write mirrors +test_io_streams.rb+'s
  # OVERFLOW_SCRIPT (past-cap write behaviour is deliberately unpinned).
  # @behavior S-038
  def test_truncation_predicate_survives_timeout_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2, stdout_limit: 5)

    error = assert_raises(Kobako::TimeoutError) do
      sandbox.eval('begin; puts "long enough to overflow the 5-byte cap"; rescue StandardError; end; loop { }')
    end

    assert_equal "long ", error.execution.stdout,
                 "stdout overflowing its cap before a TimeoutError must keep " \
                 "exactly its first stdout_limit bytes"
    assert_predicate error.execution, :stdout_truncated?,
                     "a cap overflow before the trap must stay observable " \
                     "through the rescue"
  end

  # One channel suffices here; the two-channel divergence witness is the
  # timeout case above.
  # @behavior S-039
  def test_partial_stdout_readable_after_memory_limit_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 1 << 20)

    error = assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('puts "before alloc"; a = []; 100.times { a << ("x" * 50_000) }; nil')
    end

    assert_equal "before alloc\n", error.execution.stdout,
                 "stdout written before a MemoryLimitError must stay " \
                 "readable after the rescue"
  end

  # The allocation +test_memory_limit_traps_single_invocation_past_default_cap+
  # proves traps under the default budget, so its completing here proves nil
  # reaches the limiter as unbounded rather than falling back to
  # DEFAULT_MEMORY_LIMIT.
  # @behavior S-009
  def test_nil_caps_disable_enforcement_rather_than_fall_back_to_defaults
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: nil, memory_limit: nil)

    assert_equal 100, sandbox.eval('a = []; 100.times { a << ("x" * 50_000) }; a.size').value,
                 "with both caps disabled, an allocation that traps under the default " \
                 "1 MiB cap must complete — nil disables the cap rather than falling back " \
                 "to DEFAULT_MEMORY_LIMIT"
  end
end
