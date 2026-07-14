# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the per-invocation resource caps through real mruby
# (SPEC.md B-01): the wall-clock timeout (E-19) and linear-memory delta
# (E-20) traps, their TrapError taxonomy, per-invocation re-anchoring, and
# Sandbox reusability after a trap.
class TestE2ECaps < Minitest::Test
  include E2eGuestHelper

  # SPEC.md B-01 / E-19: a wall-clock `timeout` cap interrupts an
  # infinite loop at the next guest safepoint after the deadline. The
  # cap raises `Kobako::TimeoutError`, which is a `Kobako::TrapError`
  # subclass — callers that only care about the unrecoverable outcome
  # can rescue the base class.
  def test_timeout_cap_traps_infinite_loop
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    started = Time.now
    err = assert_raises(Kobako::TimeoutError) { sandbox.eval("loop { }") }
    elapsed = Time.now - started

    assert_kind_of Kobako::TrapError, err,
                   "TimeoutError must be a TrapError subclass per SPEC.md E-19"
    assert_operator elapsed, :<, 2.0,
                    "timeout must fire within the configured budget (epoch ticker latency aside)"
    assert_match(/timeout|wall-clock/i, err.message)
  end

  # SPEC.md B-01 / E-20: `memory_limit` traps when guest `memory.grow`
  # would push the per-invocation linear-memory delta past the cap.
  # The cap measures only the growth attributable to this invocation —
  # the mruby image's initial allocation and the watermark left by
  # prior invocations sit outside the budget — so a runaway script
  # that allocates far more than the cap still surfaces as a trap.
  def test_memory_limit_cap_traps_runaway_allocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 2 << 20)

    err = assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('a = []; 200.times { a << ("x" * 100_000) }; nil')
    end

    assert_kind_of Kobako::TrapError, err,
                   "MemoryLimitError must be a TrapError subclass per SPEC.md E-20"
    assert_match(/memory_limit/, err.message)
  end

  # SPEC.md B-01 / E-20: `memory_limit` is a per-invocation delta cap,
  # re-anchored at the linear-memory size observed when each invocation
  # enters. The same Sandbox can therefore run back-to-back scripts
  # that each allocate well within the cap, even when their combined
  # high-water mark exceeds it — the watermark left by the first
  # invocation is folded into the second invocation's baseline rather
  # than being charged against its budget.
  def test_memory_limit_resets_per_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 1 << 20)

    assert_equal 200_000, sandbox.eval('a = "x" * 200_000; a.bytesize')
    assert_equal 200_000, sandbox.eval('a = "x" * 200_000; a.bytesize')
  end

  # SPEC.md B-01 / E-20: the per-invocation delta cap is enforced even
  # at the default 1 MiB budget — a single invocation whose `memory.grow`
  # delta past the entry-time baseline pushes past 1 MiB still traps,
  # complementing the 2-MiB-cap runaway case above. The exact-threshold
  # bisection lives in the cargo `KobakoLimiter` unit tests; this case
  # only pins that the cap is wired through the real guest at the
  # default cap, not at some far larger figure.
  def test_memory_limit_traps_single_invocation_past_default_cap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 1 << 20)

    err = assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('a = []; 100.times { a << ("x" * 50_000) }; nil')
    end

    assert_match(/memory_limit/, err.message)
  end

  # SPEC.md L161-173 (setup-once / run-many) + E-19: a host trap is
  # recoverable. The per-invocation cap window that `Runtime#eval` opens is
  # always closed afterwards whether the guest returns or traps, so the
  # next invocation runs under a fresh window rather than inheriting the
  # trapped run's armed deadline. The reuse-after-success path is pinned by
  # +test_memory_limit_resets_per_invocation+ and the reuse-after-guest-
  # raise path by +test_entrypoint_runtime_exception_surfaces_as_sandbox_error+;
  # this case closes the remaining gap — reuse after a host *trap*.
  def test_sandbox_reusable_after_timeout_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    assert_raises(Kobako::TimeoutError) { sandbox.eval("loop { }") }

    assert_equal 3, sandbox.eval("1 + 2"),
                 "a Sandbox must stay usable after a TimeoutError — the next " \
                 "eval must run under a fresh cap window, not re-trap on the old one"
  end

  # SPEC.md L161-173 + E-20: the MemoryLimitError counterpart of the
  # timeout-recovery case above. After the memory cap traps a runaway
  # allocation, the same Sandbox must run a within-budget script normally —
  # the limiter re-anchors its baseline per invocation rather than staying
  # armed at the trapped run's watermark.
  def test_sandbox_reusable_after_memory_limit_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 1 << 20)

    assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('a = []; 100.times { a << ("x" * 50_000) }; nil')
    end

    assert_equal 200_000, sandbox.eval('a = "x" * 200_000; a.bytesize'),
                 "a Sandbox must stay usable after a MemoryLimitError — the next " \
                 "within-budget eval must succeed under a re-anchored cap window"
  end

  # SPEC.md B-04 / E-19: the output buffers are populated on every
  # invocation outcome, so the bytes a guest wrote before a wall-clock
  # trap stay readable after the rescue. The guest writes both channels
  # in the one run — the trap kills the instance mid-invocation, the one
  # moment the two capture pipes could plausibly diverge — and this case
  # asserts the stdout half; the stderr half is the case below.
  def test_partial_stdout_readable_after_timeout_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    assert_raises(Kobako::TimeoutError) do
      sandbox.eval('$stdout.puts "out before trap"; $stderr.puts "err before trap"; loop { }')
    end

    assert_equal "out before trap\n", sandbox.stdout,
                 "stdout written before a TimeoutError must stay readable " \
                 "after the rescue per SPEC.md B-04"
    refute_predicate sandbox, :stdout_truncated?,
                     "a trap is not a cap overflow — the truncation " \
                     "predicate must stay false per SPEC.md B-04"
  end

  # SPEC.md B-04 / E-19: the stderr half of the two-channel trap run
  # above — the guest writes both channels, then traps on the wall-clock
  # cap; the bytes on stderr must survive the rescue independently.
  def test_partial_stderr_readable_after_timeout_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    assert_raises(Kobako::TimeoutError) do
      sandbox.eval('$stdout.puts "out before trap"; $stderr.puts "err before trap"; loop { }')
    end

    assert_equal "err before trap\n", sandbox.stderr,
                 "stderr written before a TimeoutError must stay readable " \
                 "after the rescue per SPEC.md B-04"
  end

  # SPEC.md B-04 / E-19: the truncation predicate keeps its meaning
  # through a trap — a channel that overflowed its cap before the trap
  # fired reports +true+ after the rescue, alongside the clipped bytes.
  # The rescued overflow write mirrors +test_io_streams.rb+'s
  # OVERFLOW_SCRIPT (past-cap write behaviour is deliberately unpinned).
  def test_truncation_predicate_survives_timeout_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2, stdout_limit: 5)

    assert_raises(Kobako::TimeoutError) do
      sandbox.eval('begin; puts "long enough to overflow the 5-byte cap"; rescue StandardError; end; loop { }')
    end

    assert_equal "long ", sandbox.stdout,
                 "stdout overflowing its cap before a TimeoutError must keep " \
                 "exactly its first stdout_limit bytes per SPEC.md B-04"
    assert_predicate sandbox, :stdout_truncated?,
                     "a cap overflow before the trap must stay observable " \
                     "through the rescue per SPEC.md B-04"
  end

  # SPEC.md B-04 / E-20: the MemoryLimitError counterpart — output written
  # before the linear-memory cap trap stays readable after the rescue.
  # One channel suffices here; the two-channel divergence witness is the
  # timeout case above.
  def test_partial_stdout_readable_after_memory_limit_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 1 << 20)

    assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('puts "before alloc"; a = []; 100.times { a << ("x" * 50_000) }; nil')
    end

    assert_equal "before alloc\n", sandbox.stdout,
                 "stdout written before a MemoryLimitError must stay " \
                 "readable after the rescue per SPEC.md B-04"
  end

  # SPEC.md B-01 / E-20: timeout: nil and memory_limit: nil disable their
  # caps. The discriminating witness — the exact allocation that
  # +test_memory_limit_traps_single_invocation_past_default_cap+ proves traps
  # under the default 1 MiB budget runs to completion with both caps off,
  # proving nil reaches the limiter as unbounded rather than silently falling
  # back to the DEFAULT_MEMORY_LIMIT the value object supplies when unset. The
  # nil readback itself is pinned at the SandboxOptions tier.
  def test_nil_caps_disable_enforcement_rather_than_fall_back_to_defaults
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: nil, memory_limit: nil)

    assert_equal 100, sandbox.eval('a = []; 100.times { a << ("x" * 50_000) }; a.size'),
                 "with both caps disabled, an allocation that traps under the default " \
                 "1 MiB cap must complete — nil disables the cap rather than falling back " \
                 "to DEFAULT_MEMORY_LIMIT"
  end
end
