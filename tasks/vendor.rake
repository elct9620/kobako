# frozen_string_literal: true

# Vendor toolchain rake task
# ==========================
#
# Fetches and unpacks the build-time toolchain into `vendor/`:
#
#   * wasi-sdk          — Clang + wasi-sysroot + libsetjmp, used to
#                         cross-compile both mruby (Stage B) and the wasm
#                         crate (Stage C). Version must be >= 26 (see
#                         SPEC.md "Implementation Standards" §setjmp/longjmp).
#   * mruby             — pinned release tarball used as the guest VM source.
#   * mruby-onig-regexp — pinned-commit tarball of the third-party regex
#                         mrbgem (Onigmo engine, vendored inside the gem
#                         tarball as `onigmo-6.2.0.tar.gz`). Loaded by
#                         `build_config/wasi.rb` via `conf.gem <path>`.
#   * onigmo-build-aux  — pinned-commit GNU config.sub / config.guess
#                         (single-file shell scripts) that overwrite the
#                         pre-wasm copies shipped inside Onigmo 6.2.0.
#                         Same pattern CRuby's wasm build uses; the actual
#                         overwrite happens in
#                         `KobakoBuildConfig.pre_extract_and_patch_onigmo!`
#                         (build_config/wasi.rb), called from the
#                         CrossBuild block right after `conf.gem` loads
#                         the mrbgem.
#
# Versions are pinned as constants in `KobakoVendor` below. Bumping a version
# is the entire upgrade workflow; no git submodule pointer dance.
#
# Idempotency: every step is a `file` task that targets a sentinel path inside
# the unpacked tree. Re-runs short-circuit when the sentinel exists.
#
# Honors `KOBAKO_VENDOR_BASE_URL` to point downloads at a local fixture during
# tests (see test/test_vendor_task.rb), and `KOBAKO_VENDOR_DIR` to relocate
# the entire vendor tree (also test-only).

require "digest"
require "fileutils"
require "open-uri"
require "net/http" # eager — DOWNLOAD_TRANSIENT_ERRORS names Net::* at module-eval

# Hoisted out of the `namespace :vendor` block so that constant definitions
# don't trigger Lint/ConstantDefinitionInBlock and are introspectable from
# the test suite without re-loading the rake DSL.
#
# TODO: KobakoVendor exceeded Metrics/ModuleLength (100 lines) after the
# download-retry helpers landed. Carry an inline disable for now; a
# follow-up should extract download / SHA verification into a sub-module
# so the cop can be re-enabled cleanly.
module KobakoVendor # rubocop:disable Metrics/ModuleLength
  ROOT       = File.expand_path("..", __dir__)
  VENDOR_DIR = (ENV["KOBAKO_VENDOR_DIR"] || File.join(ROOT, "vendor")).freeze
  CACHE_DIR  = File.join(VENDOR_DIR, ".cache").freeze

  # ---- Pinned versions ---------------------------------------------------
  # wasi-sdk: must be >= 26 for native wasm32-wasip1 setjmp/longjmp support.
  WASI_SDK_VERSION      = "26"
  WASI_SDK_MINOR        = "0"
  WASI_SDK_FULL_VERSION = "#{WASI_SDK_VERSION}.#{WASI_SDK_MINOR}".freeze

  # mruby: pinned release tarball.
  MRUBY_VERSION = "4.0.0"

  # mruby-onig-regexp: third-party mrbgem that brings Onigmo regex into the
  # guest (mruby 4.0 ships no built-in Regexp). The upstream repo has no
  # tags, so we pin a commit SHA. The gem's tarball already vendors the
  # Onigmo C source as `onigmo-6.2.0.tar.gz`, so no second tarball is
  # required.
  MRUBY_ONIG_REGEXP_COMMIT = "c97d7c1e7073bc5558986da4e2d07171f0761cc8"

  # GNU config aux scripts (config.sub / config.guess). Onigmo 6.2.0
  # ships pre-wasm copies that reject `wasm32-wasi` host triples. We
  # vendor a pinned commit from GNU savannah's config.git (same source
  # CRuby's wasm build uses, see ruby/ruby `wasm/README.md`) and copy
  # them over Onigmo's shipped scripts in `build_config/wasi.rb` before
  # `./configure` runs.
  CONFIG_AUX_COMMIT = "a2287c3041a3f2a204eb942e09c015eab00dc7dd"

  # ---- Platform detection (wasi-sdk only; mruby tarball is host-agnostic).
  WASI_SDK_PLATFORM =
    case RUBY_PLATFORM
    when /arm64-darwin|aarch64-darwin/ then "arm64-macos"
    when /x86_64-darwin/               then "x86_64-macos"
    when /aarch64-linux|arm64-linux/   then "arm64-linux"
    when /x86_64-linux/                then "x86_64-linux"
    else "x86_64-linux"
    end

  WASI_SDK_TARBALL_NAME = "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}.tar.gz".freeze
  WASI_SDK_UNPACKED_DIR = "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}".freeze

  # GitHub archive tarballs for mruby are served at
  # refs/tags/{VERSION}.tar.gz (no "mruby-" prefix in the URL filename).
  # The extracted top-level directory inside the tarball is mruby-{VERSION}/.
  MRUBY_TARBALL_NAME = "#{MRUBY_VERSION}.tar.gz".freeze
  MRUBY_UNPACKED_DIR = "mruby-#{MRUBY_VERSION}".freeze

  # GitHub archive tarballs for commits are served at
  # archive/{SHA}.tar.gz; the extracted top-level directory is
  # mruby-onig-regexp-{SHA}/.
  MRUBY_ONIG_REGEXP_TARBALL_NAME = "#{MRUBY_ONIG_REGEXP_COMMIT}.tar.gz".freeze
  MRUBY_ONIG_REGEXP_UNPACKED_DIR = "mruby-onig-regexp-#{MRUBY_ONIG_REGEXP_COMMIT}".freeze

  DEFAULT_WASI_SDK_BASE =
    "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-#{WASI_SDK_VERSION}".freeze
  DEFAULT_MRUBY_BASE = "https://github.com/mruby/mruby/archive/refs/tags"
  DEFAULT_MRUBY_ONIG_REGEXP_BASE = "https://github.com/mattn/mruby-onig-regexp/archive"
  # savannah cgit serves raw blobs as `plain/<path>?id=<sha>`; pinning to
  # a commit makes the response byte-stable (TOFU sidecar in CACHE_DIR
  # detects any drift).
  CONFIG_AUX_FILES = %w[config.sub config.guess].freeze

  WASI_SDK_FINAL          = File.join(VENDOR_DIR, "wasi-sdk").freeze
  MRUBY_FINAL             = File.join(VENDOR_DIR, "mruby").freeze
  MRUBY_ONIG_REGEXP_FINAL = File.join(VENDOR_DIR, "mruby-onig-regexp").freeze
  CONFIG_AUX_FINAL        = File.join(VENDOR_DIR, "onigmo-build-aux").freeze
  WASI_SDK_SENTINEL          = "bin/clang"
  MRUBY_SENTINEL             = "Rakefile"
  MRUBY_ONIG_REGEXP_SENTINEL = "mrbgem.rake"

  WASI_TARBALL_PATH             = File.join(CACHE_DIR, WASI_SDK_TARBALL_NAME).freeze
  MRUBY_TARBALL_PATH            = File.join(CACHE_DIR, MRUBY_TARBALL_NAME).freeze
  MRUBY_ONIG_REGEXP_TARBALL_PATH = File.join(CACHE_DIR, MRUBY_ONIG_REGEXP_TARBALL_NAME).freeze

  # When KOBAKO_VENDOR_BASE_URL is set, both tarballs are fetched from that
  # base URL (test fixture). The base URL must serve files named exactly
  # `WASI_SDK_TARBALL_NAME` and `MRUBY_TARBALL_NAME`.
  def self.base_url_for(default)
    override = ENV.fetch("KOBAKO_VENDOR_BASE_URL", nil)
    return default if override.nil? || override.empty?

    override.chomp("/")
  end

  # Expected SHA256 for a vendored tarball, sourced from
  # `KOBAKO_VENDOR_<KEY>_SHA256` env vars (empty string falls back to TOFU
  # sidecar pinning in `verify_or_pin`). +key+ is the artifact slug in
  # upper snake case, e.g. +:WASI_SDK+, +:MRUBY+, +:MRUBY_ONIG_REGEXP+.
  def self.expected_sha256(key)
    ENV.fetch("KOBAKO_VENDOR_#{key}_SHA256", "")
  end

  # -----------------------------------------------------------------------
  # Helpers
  # -----------------------------------------------------------------------

  # Retry attempts wait +2 ** attempt+ seconds (2 + 4 + 8 = 14s total)
  # — enough to ride out a GitHub archive 502 / TCP read timeout.
  DOWNLOAD_MAX_RETRIES = 3

  # Transient network errors retried by +with_download_retry+.
  # +OpenURI::HTTPError+ is narrowed to 5xx; 4xx (URL typo, deleted
  # repo) bypasses the retry path.
  DOWNLOAD_TRANSIENT_ERRORS = [
    OpenURI::HTTPError, Net::ReadTimeout, Net::OpenTimeout,
    Errno::ECONNRESET, SocketError
  ].freeze

  def self.download(url, dest)
    FileUtils.mkdir_p(File.dirname(dest))
    tmp = "#{dest}.part"
    with_download_retry do
      URI.parse(url).open("rb") { |io| File.open(tmp, "wb") { |f| IO.copy_stream(io, f) } }
    end
    File.rename(tmp, dest)
  end

  # Exponential-backoff retry wrapper for transient download failures.
  # +OpenURI::HTTPError+ is narrowed to 5xx so 4xx (URL typo, repo
  # deleted, expired ref) bypasses the retry path and surfaces
  # immediately.
  def self.with_download_retry
    attempts = 0
    begin
      yield
    rescue *DOWNLOAD_TRANSIENT_ERRORS => e
      raise if permanent_download_error?(e) || (attempts += 1) > DOWNLOAD_MAX_RETRIES

      warn_and_sleep_for_retry(e, attempts)
      retry
    end
  end

  def self.permanent_download_error?(error)
    error.is_a?(OpenURI::HTTPError) && !error.message.match?(/\A5\d\d\b/)
  end

  def self.warn_and_sleep_for_retry(error, attempt)
    warn "[vendor] retry #{attempt}/#{DOWNLOAD_MAX_RETRIES} after #{error.class}: " \
         "#{error.message.lines.first&.strip}"
    sleep(2**attempt)
  end

  def self.sha256_of(path)
    Digest::SHA256.file(path).hexdigest
  end

  # Verify the tarball against expected_sha (if non-empty) or TOFU-pin it.
  # Raises on mismatch.
  def self.verify_or_pin(path, expected_sha)
    actual = sha256_of(path)
    sidecar = "#{path}.sha256"

    if expected_sha && !expected_sha.empty?
      verify_against_expected(path, actual, expected_sha, sidecar)
    else
      verify_or_pin_sidecar(path, actual, sidecar)
    end

    actual
  end

  def self.verify_against_expected(path, actual, expected_sha, sidecar)
    unless actual == expected_sha
      raise "checksum mismatch for #{File.basename(path)}: " \
            "expected #{expected_sha}, got #{actual}"
    end
    File.write(sidecar, "#{actual}\n")
  end

  def self.verify_or_pin_sidecar(path, actual, sidecar)
    if File.exist?(sidecar)
      pinned = File.read(sidecar).strip
      return if actual == pinned

      raise "checksum drift for #{File.basename(path)}: " \
            "pinned #{pinned}, got #{actual}"
    end
    File.write(sidecar, "#{actual}\n")
  end

  # Prepare an unpacked tree at `final_dir` from `tarball`'s `top_level_dir`.
  # If `final_dir` already exists with the sentinel inside, this is a no-op.
  def self.prepare_unpacked(tarball:, top_level_dir:, final_dir:, sentinel:)
    return if File.exist?(File.join(final_dir, sentinel))

    staging = extract_to_staging(tarball, final_dir)
    src = File.join(staging, top_level_dir)
    raise "expected #{src} after extracting #{tarball}, missing" unless File.directory?(src)

    FileUtils.rm_rf(final_dir)
    FileUtils.mkdir_p(File.dirname(final_dir))
    FileUtils.mv(src, final_dir)
    FileUtils.rm_rf(staging)
  end

  def self.extract_to_staging(tarball, final_dir)
    staging = "#{final_dir}.staging"
    FileUtils.rm_rf(staging)
    FileUtils.mkdir_p(staging)
    system("tar", "-xzf", tarball, "-C", staging, exception: true)
    staging
  end
end

namespace :vendor do
  # File-level cache: tarballs land in vendor/.cache/.
  file KobakoVendor::WASI_TARBALL_PATH do |t|
    url = "#{KobakoVendor.base_url_for(KobakoVendor::DEFAULT_WASI_SDK_BASE)}/" \
          "#{KobakoVendor::WASI_SDK_TARBALL_NAME}"
    puts "[vendor] downloading wasi-sdk #{KobakoVendor::WASI_SDK_FULL_VERSION} " \
         "(#{KobakoVendor::WASI_SDK_PLATFORM}) from #{url}"
    KobakoVendor.download(url, t.name)
    KobakoVendor.verify_or_pin(t.name, KobakoVendor.expected_sha256(:WASI_SDK))
  end

  file KobakoVendor::MRUBY_TARBALL_PATH do |t|
    url = "#{KobakoVendor.base_url_for(KobakoVendor::DEFAULT_MRUBY_BASE)}/" \
          "#{KobakoVendor::MRUBY_TARBALL_NAME}"
    puts "[vendor] downloading mruby #{KobakoVendor::MRUBY_VERSION} from #{url}"
    KobakoVendor.download(url, t.name)
    KobakoVendor.verify_or_pin(t.name, KobakoVendor.expected_sha256(:MRUBY))
  end

  file KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_PATH do |t|
    url = "#{KobakoVendor.base_url_for(KobakoVendor::DEFAULT_MRUBY_ONIG_REGEXP_BASE)}/" \
          "#{KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_NAME}"
    puts "[vendor] downloading mruby-onig-regexp #{KobakoVendor::MRUBY_ONIG_REGEXP_COMMIT[0, 8]} from #{url}"
    KobakoVendor.download(url, t.name)
    KobakoVendor.verify_or_pin(t.name, KobakoVendor.expected_sha256(:MRUBY_ONIG_REGEXP))
  end

  KobakoVendor::CONFIG_AUX_FILES.each do |filename|
    commit = KobakoVendor::CONFIG_AUX_COMMIT
    cache_path = File.join(KobakoVendor::CACHE_DIR, "#{filename}.#{commit}")
    file cache_path do |t|
      url = "https://git.savannah.gnu.org/cgit/config.git/plain/#{filename}?id=#{commit}"
      puts "[vendor] downloading #{filename}@#{commit[0, 8]} from #{url}"
      KobakoVendor.download(url, t.name)
      sha_key = :"CONFIG_#{filename.split(".").last.upcase}"
      KobakoVendor.verify_or_pin(t.name, KobakoVendor.expected_sha256(sha_key))
    end
  end

  namespace :setup do
    desc "Download and unpack wasi-sdk #{KobakoVendor::WASI_SDK_FULL_VERSION} into vendor/wasi-sdk/"
    task wasi_sdk: KobakoVendor::WASI_TARBALL_PATH do
      KobakoVendor.verify_or_pin(KobakoVendor::WASI_TARBALL_PATH, KobakoVendor.expected_sha256(:WASI_SDK))
      KobakoVendor.prepare_unpacked(
        tarball: KobakoVendor::WASI_TARBALL_PATH,
        top_level_dir: KobakoVendor::WASI_SDK_UNPACKED_DIR,
        final_dir: KobakoVendor::WASI_SDK_FINAL,
        sentinel: KobakoVendor::WASI_SDK_SENTINEL
      )
      puts "[vendor] wasi-sdk ready at #{KobakoVendor::WASI_SDK_FINAL}"
    end

    desc "Download and unpack mruby #{KobakoVendor::MRUBY_VERSION} into vendor/mruby/"
    task mruby: KobakoVendor::MRUBY_TARBALL_PATH do
      KobakoVendor.verify_or_pin(KobakoVendor::MRUBY_TARBALL_PATH, KobakoVendor.expected_sha256(:MRUBY))
      KobakoVendor.prepare_unpacked(
        tarball: KobakoVendor::MRUBY_TARBALL_PATH,
        top_level_dir: KobakoVendor::MRUBY_UNPACKED_DIR,
        final_dir: KobakoVendor::MRUBY_FINAL,
        sentinel: KobakoVendor::MRUBY_SENTINEL
      )
      puts "[vendor] mruby ready at #{KobakoVendor::MRUBY_FINAL}"
    end

    desc "Download and unpack mruby-onig-regexp into vendor/mruby-onig-regexp/"
    task mruby_onig_regexp: KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_PATH do
      KobakoVendor.verify_or_pin(KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_PATH,
                                 KobakoVendor.expected_sha256(:MRUBY_ONIG_REGEXP))
      KobakoVendor.prepare_unpacked(
        tarball: KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_PATH,
        top_level_dir: KobakoVendor::MRUBY_ONIG_REGEXP_UNPACKED_DIR,
        final_dir: KobakoVendor::MRUBY_ONIG_REGEXP_FINAL,
        sentinel: KobakoVendor::MRUBY_ONIG_REGEXP_SENTINEL
      )
      puts "[vendor] mruby-onig-regexp ready at #{KobakoVendor::MRUBY_ONIG_REGEXP_FINAL}"
    end

    desc "Download GNU config.sub / config.guess into vendor/onigmo-build-aux/"
    aux_cache_paths = KobakoVendor::CONFIG_AUX_FILES.map do |f|
      File.join(KobakoVendor::CACHE_DIR, "#{f}.#{KobakoVendor::CONFIG_AUX_COMMIT}")
    end
    task onigmo_build_aux: aux_cache_paths do
      FileUtils.mkdir_p(KobakoVendor::CONFIG_AUX_FINAL)
      KobakoVendor::CONFIG_AUX_FILES.each_with_index do |filename, idx|
        dst = File.join(KobakoVendor::CONFIG_AUX_FINAL, filename)
        FileUtils.cp(aux_cache_paths[idx], dst)
        File.chmod(0o755, dst)
      end
      puts "[vendor] onigmo-build-aux ready at #{KobakoVendor::CONFIG_AUX_FINAL}"
    end
  end

  desc "Fetch and unpack all build-time vendor toolchains (wasi-sdk + mruby + mruby-onig-regexp + onigmo-build-aux)"
  task setup: ["setup:wasi_sdk", "setup:mruby", "setup:mruby_onig_regexp", "setup:onigmo_build_aux"]

  desc "Remove unpacked vendor toolchains (keeps cached tarballs)"
  task :clean do
    FileUtils.rm_rf(KobakoVendor::WASI_SDK_FINAL)
    FileUtils.rm_rf(KobakoVendor::MRUBY_FINAL)
    FileUtils.rm_rf(KobakoVendor::MRUBY_ONIG_REGEXP_FINAL)
    FileUtils.rm_rf(KobakoVendor::CONFIG_AUX_FINAL)
  end

  desc "Remove vendor/ entirely (unpacked trees and cached tarballs)"
  task :clobber do
    FileUtils.rm_rf(KobakoVendor::VENDOR_DIR)
  end
end
