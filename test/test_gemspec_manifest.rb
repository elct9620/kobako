# frozen_string_literal: true

# Intentionally does NOT require "test_helper" — that path loads the native
# extension, which isn't built in clean checkouts. This test only inspects
# the gemspec packaging pipeline and needs no Kobako Ruby code at runtime.
require "minitest/autorun"
require "open3"
require "rubygems/package"
require "tmpdir"
require "fileutils"

# E2E test: actually run `gem build kobako.gemspec`, then inspect the
# resulting .gem package's manifest and assert it matches the SPEC.md
# "Code Organization" §gemspec files whitelist exactly.
#
# This test must NOT stub `gem build` — it has to invoke the real RubyGems
# packaging pipeline, because the gemspec file allowlist is the contract
# between the repo layout and the published artifact, and any divergence
# between what the gemspec author intends and what RubyGems actually packs
# is precisely what this test exists to catch.
class TestGemspecManifest < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)
  FORBIDDEN_PREFIXES = %w[
    vendor/
    wasm/
    tasks/
    build_config/
    docs/
    benchmark/
    test/
    spec/
    bin/
    .github/
    .powerloop/
    tmp/
  ].freeze

  FORBIDDEN_EXACT = %w[
    Gemfile
    Gemfile.lock
    .gitignore
    .rubocop.yml
    SPEC.md
  ].freeze

  # Baseline `bundle gem --ext=rust` gemspec rejects the gemspec filename
  # from spec.files (rubygems still ships it via metadata.gz), so it does
  # not appear in the package's data tarball — and is not listed here.
  REQUIRED_FILES = %w[
    lib/kobako.rb
    lib/kobako/version.rb
    Rakefile
    README.md
    LICENSE
    Cargo.toml
    Cargo.lock
  ].freeze

  REQUIRED_GLOBS = [
    "ext/kobako/**/*.{rs,toml,rb,h}"
  ].freeze

  def test_built_gem_manifest_matches_spec_allowlist
    Dir.mktmpdir { |dir| assert_gem_manifest_in(dir) }
  ensure
    cleanup_stub_wasm_artifact
  end

  private

  def assert_gem_manifest_in(dir)
    stub_wasm_artifact_if_missing
    manifest = read_gem_manifest(build_gem(dir))

    refute_empty manifest, "built gem must contain at least one file"
    assert_no_forbidden_paths(manifest)
    assert_required_files_present(manifest)
    assert_required_globs_have_matches(manifest)
    assert_data_wasm_shipped(manifest)
  end

  # The published gem must include data/kobako.wasm. In a fresh dev checkout
  # the file is .gitignored and only produced by `rake compile`. To keep the
  # test self-contained, drop a tiny placeholder if and only if the real
  # artifact is absent, then clean it up at the end.
  def stub_wasm_artifact_if_missing
    wasm = File.join(PROJECT_ROOT, "data", "kobako.wasm")
    return if File.exist?(wasm)

    FileUtils.mkdir_p(File.dirname(wasm))
    File.binwrite(wasm, "\x00asm\x01\x00\x00\x00")
    @stubbed_wasm = wasm
  end

  def cleanup_stub_wasm_artifact
    File.delete(@stubbed_wasm) if @stubbed_wasm && File.exist?(@stubbed_wasm)
  end

  def build_gem(out_dir)
    out, status = Open3.capture2e(
      { "GEM_HOME" => out_dir },
      "gem", "build", "kobako.gemspec", "--output", File.join(out_dir, "kobako.gem"),
      chdir: PROJECT_ROOT
    )
    assert status.success?, "gem build failed:\n#{out}"
    File.join(out_dir, "kobako.gem")
  end

  def read_gem_manifest(gem_path)
    files = Gem::Package.new(gem_path).contents.map { |f| f }
    files.sort
  end

  def assert_no_forbidden_paths(manifest)
    bad = manifest.select do |f|
      FORBIDDEN_PREFIXES.any? { |p| f.start_with?(p) } || FORBIDDEN_EXACT.include?(f)
    end
    assert_empty bad,
                 "gem manifest contains files that must not ship per SPEC.md " \
                 "Code Organization §gemspec files whitelist: #{bad.inspect}"
  end

  def assert_required_files_present(manifest)
    missing = REQUIRED_FILES - manifest
    assert_empty missing, "gem manifest is missing required files: #{missing.inspect}"
  end

  def assert_required_globs_have_matches(manifest)
    REQUIRED_GLOBS.each do |glob|
      matched = manifest.any? { |f| File.fnmatch?(glob, f, File::FNM_PATHNAME | File::FNM_EXTGLOB) }
      assert matched, "gem manifest has no files matching required glob #{glob.inspect}"
    end
  end

  def assert_data_wasm_shipped(manifest)
    assert_includes manifest, "data/kobako.wasm",
                    "gem manifest must ship data/kobako.wasm (the pre-built Guest Binary)"
  end
end
