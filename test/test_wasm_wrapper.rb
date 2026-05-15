# frozen_string_literal: true

require "test_helper"

# Wrapper-layer tests for the sole Ruby-visible wasmtime class,
# +Kobako::Wasm::Instance+. The native ext keeps Engine, Module, and Store as
# internal Rust types — they are not reachable from Ruby (SPEC.md "Code
# Organization": `ext/` "exposes no Wasm engine types to the Host App or
# downstream gems").
#
# Scope is limited to the from_path pipeline and its error-mapping surface —
# real-guest export presence is covered transitively by the E2E journeys
# (test_e2e_journeys.rb), which drive +Sandbox#run+ end-to-end and would fail
# fast if any SPEC Wire ABI export went missing.
class TestWasmWrapper < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/minimal.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Instance)
  end

  def test_default_path_resolves_under_project_data_dir
    expected = File.expand_path("../data/kobako.wasm", __dir__)
    assert_equal expected, Kobako::Wasm.default_path
    assert Kobako::Wasm.default_path.start_with?("/"), "default_path must be absolute"
  end

  def test_from_path_raises_module_not_built_for_missing_path
    err = assert_raises(Kobako::Wasm::ModuleNotBuiltError) do
      Kobako::Wasm::Instance.from_path("/nonexistent/kobako.wasm", nil, nil)
    end
    assert_match(/rake wasm:build/, err.message)
  end

  def test_module_not_built_error_is_standard_error
    assert_operator Kobako::Wasm::ModuleNotBuiltError, :<, StandardError
    assert_operator Kobako::Wasm::ModuleNotBuiltError, :<, Kobako::Wasm::Error
  end

  def test_from_path_works_with_fixture_module
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    instance = Kobako::Wasm::Instance.from_path(FIXTURE_PATH, nil, nil)
    assert_instance_of Kobako::Wasm::Instance, instance
  end

  def test_from_path_repeated_calls_return_independent_instances
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    a = Kobako::Wasm::Instance.from_path(FIXTURE_PATH, nil, nil)
    b = Kobako::Wasm::Instance.from_path(FIXTURE_PATH, nil, nil)
    refute_same a, b, "each call must return a fresh Instance with its own Store"
  end
end
