# frozen_string_literal: true

require "test_helper"

require "digest"
require "fileutils"
require "tmpdir"

# Wrapper-layer tests for the sole Ruby-visible wasmtime class,
# +Kobako::Runtime+. The native ext keeps Engine, Module, and Store as
# internal Rust types — they are not reachable from Ruby (SPEC.md "Code
# Organization": `ext/` "exposes no Wasm engine types to the Host App or
# downstream gems").
#
# Scope is limited to the from_path pipeline and its error-mapping surface —
# real-guest export presence is covered transitively by the E2E journeys
# (test/e2e/), which drive +Sandbox#eval+ end-to-end and would fail
# fast if any SPEC Wire ABI export went missing.
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

  # docs/behavior.md B-01 Notes: the compiled-artifact disk cache is
  # best-effort — a corrupt cache entry falls back to in-process
  # compilation rather than failing construction.
  def test_from_path_falls_back_to_compile_when_cached_artifact_is_corrupt
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)

    with_private_cache_root do |dir|
      wasm_path, = plant_corrupt_artifact(dir)

      assert_instance_of Kobako::Runtime,
                         Kobako::Runtime.from_path(wasm_path, nil, nil, nil, nil),
                         "a corrupt compiled-artifact cache entry must fall back to compilation (B-01)"
    end
  end

  # The unsafe artifact deserialize trusts only a cache directory the
  # current user exclusively owns; a directory writable by group or
  # other could carry another local user's planted artifact, so both
  # disk-cache tiers must skip it — construction still succeeds via
  # in-process compilation and never writes into the lax directory.
  def test_group_writable_cache_directory_is_not_trusted
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)

    with_private_cache_root do |dir|
      wasm_path, entry = plant_corrupt_artifact(dir)
      File.chmod(0o777, File.dirname(entry))

      assert_instance_of Kobako::Runtime,
                         Kobako::Runtime.from_path(wasm_path, nil, nil, nil, nil),
                         "construction over a group-writable cache directory must fall back to compilation (B-01)"
      assert_equal "not a serialized wasmtime artifact", File.binread(entry),
                   "an artifact in a group-writable cache directory must be neither loaded nor overwritten (B-01)"
    end
  end

  # docs/behavior.md B-01 Notes: writing a new artifact opportunistically
  # removes cache entries unused for the retention window, so the cache
  # directory does not grow without bound across Guest Binary rebuilds.
  def test_storing_an_artifact_prunes_entries_unused_past_the_retention_window
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)

    with_private_cache_root do |dir|
      stale = plant_stale_artifact(dir)
      wasm_path = File.join(dir, "prune_probe.wat")
      FileUtils.cp(FIXTURE_PATH, wasm_path)

      Kobako::Runtime.from_path(wasm_path, nil, nil, nil, nil)

      refute_path_exists stale,
                         "writing a new artifact must prune cache entries unused past the retention window (B-01)"
    end
  end

  # Plant an empty cache entry whose mtime sits past the 30-day
  # retention window, so the next artifact write must remove it.
  def plant_stale_artifact(dir)
    stale = File.join(dir, "kobako", "stale.cwasm")
    FileUtils.mkdir_p(File.dirname(stale))
    FileUtils.touch(stale, mtime: Time.now - (40 * 86_400))
    stale
  end

  # Redirect the compiled-artifact cache root (B-01: XDG_CACHE_HOME) to
  # a private tmpdir for the block, so the test owns every cache entry
  # and never touches the developer's real cache.
  def with_private_cache_root
    original = ENV.fetch("XDG_CACHE_HOME", nil)
    Dir.mktmpdir do |dir|
      ENV["XDG_CACHE_HOME"] = dir
      yield dir
    end
  ensure
    ENV["XDG_CACHE_HOME"] = original
  end

  # Copy the fixture to a fresh path under +dir+ and plant garbage bytes
  # at the cache entry its content hashes to (the entry name carries the
  # gem version per B-01), so the artifact-load path is forced onto the
  # corrupt-entry branch. Returns the wasm path and the planted entry.
  def plant_corrupt_artifact(dir)
    wasm_path = File.join(dir, "corrupt_probe.wat")
    FileUtils.cp(FIXTURE_PATH, wasm_path)
    digest = Digest::SHA256.hexdigest(File.binread(wasm_path))
    entry = File.join(dir, "kobako", "#{digest}-#{Kobako::VERSION}.cwasm")
    FileUtils.mkdir_p(File.dirname(entry))
    File.write(entry, "not a serialized wasmtime artifact")
    [wasm_path, entry]
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
