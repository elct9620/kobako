# frozen_string_literal: true

require "test_helper"

# Item #14: Kobako::Sandbox.new + stdout/stderr buffers with limits.
#
# Sandbox.new constructs the wasmtime pipeline (Engine / Module / Store /
# Instance) against the test fixture wasm, owns a per-instance HandleTable,
# and creates two bounded OutputBuffers for stdout / stderr capture. The
# `#run` execution path (item #16) and the Service Registry (item #15) are
# stubbed and raise NotImplementedError.
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

  def test_default_construction_initializes_output_buffers_at_default_limit
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_instance_of Kobako::Sandbox::OutputBuffer, sandbox.stdout_buffer
    assert_instance_of Kobako::Sandbox::OutputBuffer, sandbox.stderr_buffer
    assert_equal Kobako::Sandbox::DEFAULT_OUTPUT_LIMIT, sandbox.stdout_limit
    assert_equal Kobako::Sandbox::DEFAULT_OUTPUT_LIMIT, sandbox.stderr_limit
    assert_equal 1 << 20, sandbox.stdout_limit
  end

  def test_default_construction_starts_with_empty_output_buffers
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert sandbox.stdout_buffer.empty?
    assert sandbox.stderr_buffer.empty?
  end

  def test_custom_limits_reflected
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, stdout_limit: 100, stderr_limit: 200)

    assert_equal 100, sandbox.stdout_limit
    assert_equal 200, sandbox.stderr_limit
    assert_equal 100, sandbox.stdout_buffer.limit
    assert_equal 200, sandbox.stderr_buffer.limit
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
end
