# frozen_string_literal: true

require "test_helper"

# Item #25 — Sandbox#run E2E coverage against the test-guest wasm fixture.
#
# `test/fixtures/test-guest.wasm` is built from `wasm/test-guest/` (see
# `rake fixtures:test_guest`). It implements the SPEC ABI shape exactly:
#   * `__kobako_run() -> ()` — reads Frame 1 (preamble) and Frame 2 (source)
#     from WASI stdin using the 4-byte BE u32 length-prefix protocol
#     (SPEC.md §ABI Signatures). Frame 2 bytes are decoded as a decimal
#     integer or a special keyword to drive outcome branches.
#   * `__kobako_alloc`, `__kobako_take_outcome` — as SPEC ABI.
#   * Imports `env::__kobako_rpc_call` — used by the `rpc:` source branch.
#
# These tests verify the host-side flow: preamble→source delivery via stdin
# frames → run → take_outcome → decode envelope → return value or raise.
# They are the only place outside the production Guest Binary that exercises
# the full run-path round-trip.
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
    # values — the cached wasm pipeline is reused across runs (SPEC §B-03
    # multi-run isolation is *within* a single Sandbox lifetime; the
    # Engine/Module/Store/Instance are NOT recreated, only per-run state
    # resets).
    assert_equal 1, sandbox.run("1")
    assert_equal 2, sandbox.run("2")
  end

  def test_run_reuses_engine_module_store_instance_across_runs
    # SPEC §B-03: multi-run isolation happens within a single Sandbox
    # lifetime. Engine / Module / Store / Instance are reused across runs
    # (rebuilding them is a separate use case — sandbox discard, B-19).
    # Only per-run state (HandleTable, capture buffers) is reset.
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    sandbox.run("1")
    engine_before  = sandbox.engine
    module_before  = sandbox.module_
    store_before   = sandbox.store
    instance_before = sandbox.instance

    sandbox.run("2")

    assert_same engine_before,   sandbox.engine
    assert_same module_before,   sandbox.module_
    assert_same store_before,    sandbox.store
    assert_same instance_before, sandbox.instance
  end

  def test_run_resets_handle_table_counter_to_one_between_runs
    # SPEC §B-15: Handle id counter resets to 1 at the start of every
    # #run. Run #1 burns through ids 1..N; run #2's first alloc must
    # return id 1 (not N+1).
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.run("1")
    sandbox.handle_table.alloc(:from_run_one) # id = 1 within run-1 scope
    sandbox.handle_table.alloc(:from_run_one_again) # id = 2

    sandbox.run("2")

    # After run #2's per-run reset, the next alloc must hand out id=1.
    assert_equal 1, sandbox.handle_table.alloc(:fresh_from_run_two)
  end

  def test_run_clears_stdout_buffer_between_runs
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    sandbox.run("1")
    # Inject a sentinel that must NOT survive across the run boundary.
    sandbox.stdout_buffer << "from-between-runs"
    assert_includes sandbox.stdout_buffer.to_s, "from-between-runs"

    sandbox.run("2")

    # After the second run the per-run reset cleared the sentinel; the buffer
    # now holds only what the guest wrote during run #2 (WASI capture, B-04).
    # Stderr must be empty since the fixture only writes to stdout.
    refute_includes sandbox.stdout_buffer.to_s, "from-between-runs",
                    "stdout must not retain data injected between runs"
    assert_equal "", sandbox.stderr_buffer.to_s
  end

  def test_run_one_returned_handle_is_invalid_after_run_two
    # SPEC §B-19: Handle ids issued during run N are invalid in run N+1.
    # The fixture's "handle:7" source emits a Result envelope carrying
    # ext 0x01 Handle(7). Run #1 surfaces that to the host as a
    # `Kobako::Wire::Handle`; the per-run reset on run #2 clears the
    # HandleTable, so attempting to fetch id=7 raises.
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    handle_value = sandbox.run("handle:7")
    assert_kind_of Kobako::Wire::Handle, handle_value
    assert_equal 7, handle_value.id

    # Stage id=7 in the HandleTable as if the wire layer had registered
    # it during run #1. Run #2 must invalidate it.
    sandbox.handle_table.instance_variable_get(:@entries)[7] = :run_one_capability
    assert sandbox.handle_table.include?(7)

    sandbox.run("2")

    refute sandbox.handle_table.include?(7),
           "HandleTable still bound id=7 from run #1 after run #2 reset"
    assert_raises(Kobako::HandleTableError) { sandbox.handle_table.fetch(7) }
  end

  def test_run_does_not_reset_service_registry_bindings_across_runs
    # SPEC.md §Architecture: Registry is sealed after first #run, but
    # binding entries (host-side capability declarations) persist across
    # runs — they are not "per-run state" in the multi-run isolation
    # sense. Verify both seal and persistence.
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.define(:Persistent).bind(:KV, :a_capability)

    sandbox.run("1")
    sandbox.run("2")

    assert sandbox.services.sealed?, "registry must remain sealed across runs"
    assert sandbox.services.bound?("Persistent::KV"),
           "binding declared before first #run must survive subsequent runs"
    assert_equal :a_capability, sandbox.services.lookup("Persistent::KV")
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

    # The per-run reset clears "leftover" before the guest runs. The buffer
    # afterwards holds only what the guest wrote via WASI stdout (B-04).
    # We assert the sentinel is gone; we do NOT assert the buffer is empty
    # because the test-guest fixture now writes a marker to stdout on every run.
    refute_includes sandbox.stdout_buffer.to_s, "leftover",
                    "stdout must not retain pre-run leftover data"
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

  # Item #25 — Frame 1 preamble is parsed without error when Service Members
  # are bound. The fixture consumes Frame 1 and discards it; Frame 2 drives
  # the outcome. Verifies that preamble delivery does not break the run path.
  def test_run_with_bound_service_members_still_returns_correct_value
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    grp = sandbox.define(:MyService)
    grp.bind(:KV, Object.new)
    grp.bind(:Logger, Object.new)

    assert_equal 42, sandbox.run("42")
  end

  # Item #25 — An empty registry (no groups) produces a valid empty Frame 1
  # payload (`[]` msgpack array). Frame 2 delivery and outcome still work.
  def test_run_with_empty_registry_produces_valid_frame1
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal 7, sandbox.run("7")
  end

  # Item #25 — Repeated runs with a sealed registry reuse the same preamble
  # bytes per run. Verifies multi-run isolation holds under the new stdin path.
  def test_run_with_preamble_survives_multiple_runs
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.define(:Auth).bind(:Token, Object.new)

    assert_equal 1, sandbox.run("1")
    assert_equal 2, sandbox.run("2")
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
