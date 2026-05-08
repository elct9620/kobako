# frozen_string_literal: true

require "test_helper"

# Item #16 — Sandbox#run E2E coverage against the test-guest wasm fixture.
#
# `test/fixtures/test-guest.wasm` is built from `wasm/test-guest/` (see
# `rake fixtures:test_guest`). It implements the SPEC ABI surface with
# stub bodies that:
#   * accept the source bytes via `__kobako_run(ptr, len)` (a deliberate
#     deviation from SPEC `() -> ()`; the WASI-stdin path lands later),
#   * decode the bytes as a decimal integer and emit a Result envelope
#     carrying that integer — except when the source is the literal
#     string "panic", which emits a Panic envelope.
#
# These tests verify the host-side flow: alloc → write source → run →
# take_outcome → decode envelope → return value (or raise the right
# error). They are the only place outside the production Guest Binary
# that exercises the full run-path round-trip.
class TestSandboxRun < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/test-guest.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Engine)
    skip "test-guest fixture missing (run `bundle exec rake fixtures:test_guest`)" \
      unless File.exist?(FIXTURE_PATH)
  end

  def test_run_returns_integer_value_from_result_envelope
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal 42, sandbox.run("42")
  end

  def test_run_returns_different_value_for_different_source
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal 99, sandbox.run("99")
  end

  def test_run_supports_consecutive_runs_on_same_sandbox
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    # Two consecutive #run calls on the same Sandbox both return correct
    # values. (Full multi-run state isolation — capability state, fresh
    # instance — lands with item #17; this asserts the lighter property
    # that the existing instance survives a second invocation.)
    assert_equal 1, sandbox.run("1")
    assert_equal 2, sandbox.run("2")
  end

  def test_run_raises_sandbox_error_for_panic_with_sandbox_origin
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    err = assert_raises(Kobako::SandboxError) { sandbox.run("panic") }
    assert_equal "boom", err.message
    assert_equal "sandbox", err.origin
    assert_equal "RuntimeError", err.klass
    assert_equal ["test-guest:1"], err.backtrace_lines
  end

  def test_run_clears_buffers_between_invocations
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.stdout_buffer << "leftover"

    sandbox.run("7")

    # The current test fixture does not exercise WASI stdout/stderr —
    # real WASI capture lands later. We only assert the per-run reset
    # invariant from B-03/B-04.
    assert_equal "", sandbox.stdout_buffer.to_s
    assert_equal "", sandbox.stderr_buffer.to_s
  end

  def test_run_resets_handle_table_between_invocations
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.handle_table.alloc(:from_setup)
    refute_equal 0, sandbox.handle_table.size

    sandbox.run("5")

    assert_equal 0, sandbox.handle_table.size
  end

  def test_run_seals_service_registry_on_first_call
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.define(:Early).bind(:Member, :pre_run)

    sandbox.run("3")

    assert sandbox.services.sealed?
    assert_raises(ArgumentError) { sandbox.define(:Late) }
  end

  def test_run_rejects_non_string_source
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    err = assert_raises(Kobako::SandboxError) { sandbox.run(123) }
    assert_match(/must be a String/, err.message)
  end
end

# Real-tier E2E — runs only when KOBAKO_E2E_BUILD=1 is set AND the heavy
# `data/kobako.wasm` artifact has been built (rake wasm:guest). Skipped in
# normal lanes because the build chain (vendor + mruby + cargo) is slow.
class TestSandboxRunRealTier < Minitest::Test
  REAL_WASM = File.expand_path("../data/kobako.wasm", __dir__)

  def setup
    skip "set KOBAKO_E2E_BUILD=1 to run real-tier sandbox#run coverage" \
      unless ENV["KOBAKO_E2E_BUILD"] == "1"
    skip "data/kobako.wasm missing (run `bundle exec rake wasm:guest`)" \
      unless File.exist?(REAL_WASM)
    skip "native ext not compiled" unless defined?(Kobako::Wasm::Engine)
  end

  def test_real_guest_returns_value_from_simple_expression
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    # Real mruby integration; expression semantics depend on the boot
    # script + mruby. This is a smoke assertion — exact value contract
    # belongs in item #17+ once the production guest path stabilises.
    refute_nil sandbox.run("1 + 1")
  end
end
