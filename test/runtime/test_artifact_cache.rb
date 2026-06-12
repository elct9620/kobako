# frozen_string_literal: true

require "test_helper"

require "digest"
require "fileutils"
require "tmpdir"

# Compiled-artifact disk cache behaviour through +Kobako::Runtime.from_path+
# (docs/behavior.md B-01 Notes): the cache is best-effort — corrupt
# entries fall back to in-process compilation, untrusted directories are
# skipped, and writes prune entries unused past the retention window.
class TestRuntimeArtifactCache < Minitest::Test
  FIXTURE_PATH = File.expand_path("../fixtures/minimal_abi_ok.wat", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)
  end

  # docs/behavior.md B-01 Notes: a corrupt cache entry falls back to
  # in-process compilation rather than failing construction.
  def test_from_path_falls_back_to_compile_when_cached_artifact_is_corrupt
    with_private_cache_root do |dir|
      wasm_path, entry = plant_corrupt_artifact(dir)

      assert_instance_of Kobako::Runtime,
                         Kobako::Runtime.from_path(wasm_path, nil, nil, nil, nil),
                         "a corrupt compiled-artifact cache entry must fall back to compilation (B-01)"
      # Key-derivation witness: the test and the ext name the entry
      # independently — a drift would leave it unconsulted and vacuous.
      refute_equal "not a serialized wasmtime artifact", File.binread(entry),
                   "the fallback compile must overwrite the corrupt cache entry in place (B-01)"
    end
  end

  # The unsafe artifact deserialize trusts only a cache directory the
  # current user exclusively owns; a directory writable by group or
  # other could carry another local user's planted artifact, so both
  # disk-cache tiers must skip it.
  def test_group_writable_cache_directory_is_not_trusted
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
    with_private_cache_root do |dir|
      stale = plant_stale_artifact(dir)
      wasm_path = File.join(dir, "prune_probe.wat")
      FileUtils.cp(FIXTURE_PATH, wasm_path)

      Kobako::Runtime.from_path(wasm_path, nil, nil, nil, nil)

      refute_path_exists stale,
                         "writing a new artifact must prune cache entries unused past the retention window (B-01)"
    end
  end

  private

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

  # Plant an empty cache entry whose mtime sits past the 30-day
  # retention window, so the next artifact write must remove it.
  def plant_stale_artifact(dir)
    stale = File.join(dir, "kobako", "stale.cwasm")
    FileUtils.mkdir_p(File.dirname(stale))
    FileUtils.touch(stale, mtime: Time.now - (40 * 86_400))
    stale
  end
end
