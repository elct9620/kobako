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
# (test/e2e/), which drive +Sandbox#eval+ end-to-end and would fail
# fast if any SPEC Wire ABI export went missing; the compiled-artifact
# disk cache has its own class in test_artifact_cache.rb.
class TestRuntime < Minitest::Test
  FIXTURE_PATH = File.expand_path("../fixtures/minimal_abi_ok.wat", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
  end

  def test_default_path_resolves_under_project_data_dir
    expected = File.expand_path("../../data/kobako.wasm", __dir__)
    assert_equal expected, Kobako::Runtime.default_path
    assert Kobako::Runtime.default_path.start_with?("/"), "default_path must be absolute"
  end

  def test_from_path_raises_module_not_built_for_missing_path
    err = assert_raises(Kobako::ModuleNotBuiltError) do
      Kobako::Runtime.from_path("/nonexistent/kobako.wasm", nil, nil, nil, nil)
    end
    assert_match(/rake wasm:build/, err.message)
  end

  def test_from_path_works_with_fixture_module
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)

    runtime = Kobako::Runtime.from_path(FIXTURE_PATH, nil, nil, nil, nil)
    assert_instance_of Kobako::Runtime, runtime
  end

  def test_from_path_repeated_calls_return_independent_instances
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)

    a = Kobako::Runtime.from_path(FIXTURE_PATH, nil, nil, nil, nil)
    b = Kobako::Runtime.from_path(FIXTURE_PATH, nil, nil, nil, nil)
    refute_same a, b, "each call must return a fresh Runtime with its own Store"
  end

  # SPEC error taxonomy contract (docs/behavior.md E-40 / E-41): a
  # present-but-unparseable wasm artifact passing through +from_path+ raises
  # +Kobako::SetupError+, not the absent-artifact subclass
  # +ModuleNotBuiltError+ (reserved for "file absent", E-40) and not the
  # invocation-outcome +TrapError+. Construction fails before any guest
  # invocation runs, so it sits outside the invocation attribution pipeline;
  # a single +rescue Kobako::SetupError+ covers every unconstructable-runtime
  # cause — unreadable bytes, an invalid module, or instantiation failure.
  def test_from_path_raises_setup_error_for_corrupt_wasm_payload
    # Any present file whose bytes are not a valid wasm module reaches
    # the WtModule::new compile path and trips +setup_err+. Pick a small
    # fixture that ships in the repo so the test is deterministic and
    # the failure mode is "bytes are not wasm" rather than I/O.
    non_wasm = File.expand_path("../fixtures/snippet_answers.rb", __dir__)
    skip "snippet_answers.rb fixture missing" unless File.exist?(non_wasm)

    err = assert_raises(Kobako::SetupError) do
      Kobako::Runtime.from_path(non_wasm, nil, nil, nil, nil)
    end
    refute_kind_of Kobako::ModuleNotBuiltError, err,
                   "a present-but-corrupt artifact is a SetupError, not the absent-artifact subclass"
    refute_kind_of Kobako::TrapError, err,
                   "a construction failure must not be attributed as an invocation TrapError"
    assert_match(/failed to compile Sandbox runtime/, err.message)
  end

  # docs/behavior.md E-39: an invalid timeout argument is a Host App
  # programming error, raised as +ArgumentError+ before any engine work —
  # distinct from the construction-failure +SetupError+ branch. The
  # +Kobako::Sandbox+ path validates via +SandboxOptions+; this exercises the
  # ext's defence-in-depth guard on a direct +from_path+ call.
  def test_from_path_raises_argument_error_for_invalid_timeout
    err = assert_raises(ArgumentError) do
      Kobako::Runtime.from_path(Kobako::Runtime.default_path, -1.0, nil, nil, nil)
    end
    assert_match(/timeout must be > 0/, err.message)
  end
end
