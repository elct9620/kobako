# frozen_string_literal: true

require "test_helper"

# Item #12: wasmtime Engine/Module/Store/Instance wrapper smoke tests.
#
# Fast tier — runs against a hand-encoded test fixture wasm
# (test/fixtures/minimal.wasm), so no `rake wasm:guest` build is required.
# The fixture is the smallest valid module that exposes one export, giving
# us coverage of the full Engine -> Module -> Store -> Instance pipeline
# plus an export lookup, without depending on the full guest binary.
#
# Real tier — gated on KOBAKO_E2E_BUILD=1; loads the actual data/kobako.wasm
# produced by item #11 and asserts the three guest exports plus the
# `__kobako_rpc_call` host import surface line up with Ch.4 §Wire ABI.
class TestWasmWrapper < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/minimal.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Engine)
  end

  def test_engine_new_returns_instance
    engine = Kobako::Wasm::Engine.new
    assert_instance_of Kobako::Wasm::Engine, engine
  end

  def test_default_path_resolves_under_project_data_dir
    expected = File.expand_path("../data/kobako.wasm", __dir__)
    assert_equal expected, Kobako::Wasm.default_path
    assert Kobako::Wasm.default_path.start_with?("/"), "default_path must be absolute"
  end

  def test_module_from_file_raises_when_missing
    engine = Kobako::Wasm::Engine.new
    err = assert_raises(Kobako::Wasm::ModuleNotBuiltError) do
      Kobako::Wasm::Module.from_file(engine, "/nonexistent/kobako.wasm")
    end
    assert_match(/rake wasm:guest/, err.message)
  end

  def test_module_not_built_error_is_standard_error
    assert_operator Kobako::Wasm::ModuleNotBuiltError, :<, StandardError
    assert_operator Kobako::Wasm::ModuleNotBuiltError, :<, Kobako::Wasm::Error
  end

  def test_full_pipeline_with_fixture
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    mod, store, instance = build_full_pipeline(FIXTURE_PATH)

    assert_instance_of Kobako::Wasm::Module, mod
    assert_instance_of Kobako::Wasm::Store, store
    assert_instance_of Kobako::Wasm::Instance, instance
    assert instance.has_export?("ping"), "fixture must expose `ping` export"
    refute instance.has_export?("__kobako_run"), "fixture must NOT expose guest binary exports"
    assert_equal 0, store.rpc_call_count, "no RPC calls expected before guest invocation"
  end

  # Engine is the inner construction dependency — held only inside this
  # helper since the test asserts on the externally-visible mod / store /
  # instance triple.
  def build_full_pipeline(path)
    engine = Kobako::Wasm::Engine.new
    mod = Kobako::Wasm::Module.from_file(engine, path)
    store = Kobako::Wasm::Store.new(engine)
    instance = Kobako::Wasm::Instance.new(engine, mod, store)
    [mod, store, instance]
  end

  def test_real_guest_binary_when_built
    skip "set KOBAKO_E2E_BUILD=1 to run the real-tier wasm wrapper test" unless ENV["KOBAKO_E2E_BUILD"] == "1"
    skip "data/kobako.wasm not built; run `bundle exec rake wasm:guest`" unless File.exist?(Kobako::Wasm.default_path)

    engine = Kobako::Wasm::Engine.new
    mod = Kobako::Wasm::Module.from_file(engine, Kobako::Wasm.default_path)
    store = Kobako::Wasm::Store.new(engine)
    instance = Kobako::Wasm::Instance.new(engine, mod, store)

    # All three Wire ABI exports must be present (per Ch.4 §Wire ABI exports).
    assert instance.has_export?("__kobako_run"),          "guest binary must export __kobako_run"
    assert instance.has_export?("__kobako_take_outcome"), "guest binary must export __kobako_take_outcome"
    assert instance.has_export?("__kobako_alloc"),        "guest binary must export __kobako_alloc"
    assert_equal 3, instance.known_export_count
  end
end
