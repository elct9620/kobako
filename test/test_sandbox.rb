# frozen_string_literal: true

require "test_helper"

# Item #14: Kobako::Sandbox.new + stdout/stderr capture with limits.
#
# Sandbox.new constructs the wasmtime pipeline (Engine / Module / Store /
# Instance) against the test fixture wasm, owns a per-instance HandleTable,
# and holds the per-channel byte caches that back `#stdout` / `#stderr` /
# `#stdout_truncated?` / `#stderr_truncated?` (SPEC.md B-04). The per-
# channel cap itself is enforced inside the ext-owned WASI pipe.
class TestSandbox < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/minimal.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Instance)
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)
  end

  def test_default_construction_wires_wasm_pipeline_and_handle_table
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal FIXTURE_PATH, sandbox.wasm_path
    assert_instance_of Kobako::Wasm::Instance, sandbox.instance
    assert_instance_of Kobako::Registry::HandleTable, sandbox.services.handle_table
  end

  def test_default_construction_exposes_default_output_limits
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal Kobako::Sandbox::DEFAULT_OUTPUT_LIMIT, sandbox.stdout_limit
    assert_equal Kobako::Sandbox::DEFAULT_OUTPUT_LIMIT, sandbox.stderr_limit
    assert_equal 1 << 20, sandbox.stdout_limit
  end

  # SPEC.md B-05: reading the capture channels before any +#run+ returns
  # an empty UTF-8 String; the truncation predicates default to +false+.
  def test_pre_run_capture_state_matches_b05
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal "", sandbox.stdout
    assert_equal "", sandbox.stderr
    assert_equal Encoding::UTF_8, sandbox.stdout.encoding
    assert_equal Encoding::UTF_8, sandbox.stderr.encoding
    refute sandbox.stdout_truncated?
    refute sandbox.stderr_truncated?
  end

  def test_custom_limits_reflected
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, stdout_limit: 100, stderr_limit: 200)

    assert_equal 100, sandbox.stdout_limit
    assert_equal 200, sandbox.stderr_limit
  end

  def test_missing_wasm_raises_module_not_built_error
    assert_raises(Kobako::Wasm::ModuleNotBuiltError) do
      Kobako::Sandbox.new(wasm_path: "/nonexistent/kobako.wasm")
    end
  end

  def test_handle_tables_have_distinct_identity_per_sandbox
    a = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    b = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    refute_same a.services.handle_table, b.services.handle_table
  end

  def test_handle_table_alloc_does_not_leak_across_sandboxes
    a = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    b = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    a.services.handle_table.alloc(:x)
    a.services.handle_table.alloc(:y)

    assert_equal 2, a.services.handle_table.size
    assert_equal 0, b.services.handle_table.size, "alloc on one Sandbox must not leak to another"
  end

  def test_run_against_minimal_fixture_raises_trap_error_when_run_missing
    # The minimal.wasm fixture has none of the SPEC ABI exports, so the
    # run step raises Kobako::Wasm::Error which `#run` re-wraps as a
    # TrapError. Source delivery is via WASI stdin frames now, so the first
    # ext call is `__kobako_run` (not alloc). Real fixture-based E2E coverage
    # lives in test/test_sandbox_run.rb.
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    err = assert_raises(Kobako::TrapError) { sandbox.run("nil") }
    assert_match(/__kobako_run/, err.message)
  end

  def test_run_rejects_non_string_source
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    err = assert_raises(Kobako::SandboxError) { sandbox.run(nil) }
    assert_match(/must be a String/, err.message)
  end

  def test_services_attribute_is_real_registry
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    assert_instance_of Kobako::Registry, sandbox.services
    assert sandbox.services.empty?
    group = sandbox.services.define(:Foo)
    assert_instance_of Kobako::Registry::ServiceGroup, group
  end

  # SPEC.md B-01: `timeout` defaults to 60 s (Float), `memory_limit`
  # to 5 MiB. Both surface as readonly attributes for introspection
  # by Host Apps that need to log policy. Pin the literal SPEC values
  # so the test catches a drift in either direction.
  def test_default_caps_match_spec_b01
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal 60.0, sandbox.timeout
    assert_equal 5 << 20, sandbox.memory_limit
  end

  def test_custom_caps_reflected
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: 1.5, memory_limit: 2 << 20)

    assert_in_delta 1.5, sandbox.timeout, 1e-9
    assert_equal 2 << 20, sandbox.memory_limit
  end

  def test_nil_caps_disable_enforcement
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: nil, memory_limit: nil)

    assert_nil sandbox.timeout
    assert_nil sandbox.memory_limit
  end

  def test_integer_timeout_is_coerced_to_float
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: 5)

    assert_kind_of Float, sandbox.timeout
    assert_equal 5.0, sandbox.timeout
  end

  def test_invalid_timeout_raises_argument_error
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: 0) }
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: -1.0) }
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: "60") }
  end

  def test_invalid_memory_limit_raises_argument_error
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, memory_limit: 0) }
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, memory_limit: -1) }
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, memory_limit: 1.5) }
  end
end
