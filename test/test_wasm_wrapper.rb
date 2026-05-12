# frozen_string_literal: true

require "test_helper"

# Wrapper-layer tests for the sole Ruby-visible wasmtime class,
# +Kobako::Wasm::Instance+. The native ext keeps Engine, Module, and Store as
# internal Rust types — they are not reachable from Ruby (SPEC.md "Code
# Organization": `ext/` "exposes no Wasm engine types to the Host App or
# downstream gems").
#
# Fast tier — runs against a hand-encoded test fixture wasm
# (test/fixtures/minimal.wasm), so no `rake wasm:guest` build is required.
# The fixture is the smallest valid module that exposes one export, giving
# us coverage of the from_path pipeline plus an export lookup, without
# depending on the full guest binary.
#
# Real tier — runs when data/kobako.wasm exists (built by `rake wasm:guest`,
# which the default test task now pulls in as a prerequisite). Asserts the
# three guest exports line up with SPEC.md Wire ABI.
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
      Kobako::Wasm::Instance.from_path("/nonexistent/kobako.wasm")
    end
    assert_match(/rake wasm:guest/, err.message)
  end

  def test_module_not_built_error_is_standard_error
    assert_operator Kobako::Wasm::ModuleNotBuiltError, :<, StandardError
    assert_operator Kobako::Wasm::ModuleNotBuiltError, :<, Kobako::Wasm::Error
  end

  def test_from_path_works_with_fixture_module
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    instance = Kobako::Wasm::Instance.from_path(FIXTURE_PATH)
    assert_instance_of Kobako::Wasm::Instance, instance
    assert instance.has_export?("ping"), "fixture must expose `ping` export"
    refute instance.has_export?("__kobako_run"), "fixture must NOT expose guest binary exports"
  end

  def test_from_path_repeated_calls_return_independent_instances
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    a = Kobako::Wasm::Instance.from_path(FIXTURE_PATH)
    b = Kobako::Wasm::Instance.from_path(FIXTURE_PATH)
    refute_same a, b, "each call must return a fresh Instance with its own Store"
    assert a.has_export?("ping")
    assert b.has_export?("ping")
  end

  def test_real_guest_binary_exports_match_wire_abi
    skip "data/kobako.wasm not built; run `bundle exec rake wasm:guest`" unless File.exist?(Kobako::Wasm.default_path)

    instance = Kobako::Wasm::Instance.from_path(Kobako::Wasm.default_path)

    # All three Wire ABI exports must be present (per SPEC.md Wire ABI exports).
    assert instance.has_export?("__kobako_run"),          "guest binary must export __kobako_run"
    assert instance.has_export?("__kobako_take_outcome"), "guest binary must export __kobako_take_outcome"
    assert instance.has_export?("__kobako_alloc"),        "guest binary must export __kobako_alloc"
    assert_equal 3, instance.known_export_count
  end
end
