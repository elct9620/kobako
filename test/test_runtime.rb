# frozen_string_literal: true

require "test_helper"

# Wrapper-layer tests for the sole Ruby-visible wasmtime class,
# +Kobako::Runtime+. The native ext keeps Engine, Module, and Store as
# internal Rust types — they are not reachable from Ruby (SPEC.md "Code
# Organization": `ext/` "exposes no Wasm engine types to the Host App or
# downstream gems").
#
# Scope is limited to the from_path pipeline and its error-mapping surface —
# real-guest export presence is covered transitively by the E2E journeys
# (test_e2e_journeys.rb), which drive +Sandbox#eval+ end-to-end and would fail
# fast if any SPEC Wire ABI export went missing.
class TestRuntime < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/minimal.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
  end

  def test_default_path_resolves_under_project_data_dir
    expected = File.expand_path("../data/kobako.wasm", __dir__)
    assert_equal expected, Kobako::Runtime.default_path
    assert Kobako::Runtime.default_path.start_with?("/"), "default_path must be absolute"
  end

  def test_from_path_raises_module_not_built_for_missing_path
    err = assert_raises(Kobako::ModuleNotBuiltError) do
      Kobako::Runtime.from_path("/nonexistent/kobako.wasm", nil, nil, nil, nil)
    end
    assert_match(/rake wasm:build/, err.message)
  end

  def test_module_not_built_error_is_standard_error
    assert_operator Kobako::ModuleNotBuiltError, :<, StandardError
    assert_operator Kobako::ModuleNotBuiltError, :<, Kobako::Error
  end

  def test_from_path_works_with_fixture_module
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    runtime = Kobako::Runtime.from_path(FIXTURE_PATH, nil, nil, nil, nil)
    assert_instance_of Kobako::Runtime, runtime
  end

  def test_from_path_repeated_calls_return_independent_instances
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    a = Kobako::Runtime.from_path(FIXTURE_PATH, nil, nil, nil, nil)
    b = Kobako::Runtime.from_path(FIXTURE_PATH, nil, nil, nil, nil)
    refute_same a, b, "each call must return a fresh Runtime with its own Store"
  end

  # SPEC error taxonomy contract: a present-but-unparseable wasm artifact
  # passing through +from_path+ raises +Kobako::TrapError+, not
  # +ModuleNotBuiltError+. ModuleNotBuiltError is reserved for "file
  # absent" (the pre-+rake compile+ state operators are most likely to
  # hit); every other engine-side construction failure — wasmtime
  # rejecting the bytes, missing required exports, instantiation traps —
  # collapses to +TrapError+ so the Host App's "discard the Sandbox and
  # rebuild" recovery path covers them all under one rescue.
  def test_from_path_raises_trap_error_for_corrupt_wasm_payload
    # Any present file whose bytes are not a valid wasm module reaches
    # the WtModule::new compile path and trips +trap_err+. Pick a small
    # fixture that ships in the repo so the test is deterministic and
    # the failure mode is "bytes are not wasm" rather than I/O.
    non_wasm = File.expand_path("fixtures/snippet_answers.rb", __dir__)
    skip "snippet_answers.rb fixture missing" unless File.exist?(non_wasm)

    err = assert_raises(Kobako::TrapError) do
      Kobako::Runtime.from_path(non_wasm, nil, nil, nil, nil)
    end
    assert_match(/failed to compile Sandbox runtime/, err.message)
  end
end
