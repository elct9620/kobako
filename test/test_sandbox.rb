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
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Engine)
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)
  end

  def test_default_construction_with_fixture
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal FIXTURE_PATH, sandbox.wasm_path
    assert_instance_of Kobako::Wasm::Engine, sandbox.engine
    assert_instance_of Kobako::Wasm::Module, sandbox.module_
    assert_instance_of Kobako::Wasm::Store, sandbox.store
    assert_instance_of Kobako::Wasm::Instance, sandbox.instance
    assert_instance_of Kobako::HandleTable, sandbox.handle_table
    assert_instance_of Kobako::Sandbox::OutputBuffer, sandbox.stdout_buffer
    assert_instance_of Kobako::Sandbox::OutputBuffer, sandbox.stderr_buffer
    assert_equal Kobako::Sandbox::DEFAULT_OUTPUT_LIMIT, sandbox.stdout_limit
    assert_equal Kobako::Sandbox::DEFAULT_OUTPUT_LIMIT, sandbox.stderr_limit
    assert_equal 1 << 20, sandbox.stdout_limit
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

  def test_handle_table_is_per_sandbox_instance
    a = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    b = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    refute_same a.handle_table, b.handle_table
    a.handle_table.alloc(:x)
    a.handle_table.alloc(:y)
    assert_equal 2, a.handle_table.size
    assert_equal 0, b.handle_table.size, "alloc on one Sandbox must not leak to another"
  end

  def test_output_buffer_enforces_limit_on_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, stdout_limit: 8, stderr_limit: 8)

    sandbox.stdout_buffer << "1234567" # 7 bytes, under limit
    assert_equal 7, sandbox.stdout_buffer.bytesize

    sandbox.stdout_buffer << "8" # at limit (8 bytes total)
    assert_equal 8, sandbox.stdout_buffer.bytesize

    err = assert_raises(Kobako::Sandbox::OutputLimitExceeded) do
      sandbox.stdout_buffer << "9"
    end
    assert_match(/output limit exceeded/, err.message)
    assert_equal 8, sandbox.stdout_buffer.bytesize, "buffer must not grow on overflow"
  end

  def test_output_buffer_enforces_limit_on_stderr
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, stderr_limit: 4)

    sandbox.stderr_buffer << "abcd"
    assert_raises(Kobako::Sandbox::OutputLimitExceeded) do
      sandbox.stderr_buffer << "e"
    end
  end

  def test_output_buffer_clear_resets_to_empty
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    sandbox.stdout_buffer << "hello"
    refute sandbox.stdout_buffer.empty?
    assert_equal "hello", sandbox.stdout_buffer.to_s

    sandbox.stdout_buffer.clear
    assert sandbox.stdout_buffer.empty?
    assert_equal "", sandbox.stdout_buffer.to_s
  end

  def test_run_raises_not_implemented_with_item_16_message
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    err = assert_raises(NotImplementedError) { sandbox.run("nil") }
    assert_match(/item #16/, err.message)
  end

  def test_services_attribute_is_real_registry
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    assert_instance_of Kobako::Service::Registry, sandbox.services
    assert sandbox.services.empty?
    group = sandbox.services.define(:Foo)
    assert_instance_of Kobako::Service::Group, group
  end
end
