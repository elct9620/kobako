# frozen_string_literal: true

# Vendor toolchain rake task
# ==========================
#
# Fetches and unpacks the build-time toolchain into `vendor/`:
#
#   * wasi-sdk  — Clang + wasi-sysroot + libsetjmp, used to cross-compile both
#                 mruby (Stage B) and the wasm crate (Stage C). Version must be
#                 >= 26 (see SPEC.md "Implementation Standards" §setjmp/longjmp,
#                 and tmp/REFERENCE.md Ch.5 §setjmp/longjmp 啟用).
#   * mruby     — pinned release tarball used as the guest VM source tree.
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

# Hoisted out of the `namespace :vendor` block so that constant definitions
# don't trigger Lint/ConstantDefinitionInBlock and are introspectable from
# the test suite without re-loading the rake DSL.
module KobakoVendor
  ROOT       = File.expand_path("..", __dir__)
  VENDOR_DIR = (ENV["KOBAKO_VENDOR_DIR"] || File.join(ROOT, "vendor")).freeze
  CACHE_DIR  = File.join(VENDOR_DIR, ".cache").freeze

  # ---- Pinned versions ---------------------------------------------------
  # wasi-sdk: must be >= 26 for native wasm32-wasip1 setjmp/longjmp support.
  WASI_SDK_VERSION      = "26"
  WASI_SDK_MINOR        = "0"
  WASI_SDK_FULL_VERSION = "#{WASI_SDK_VERSION}.#{WASI_SDK_MINOR}".freeze

  # mruby: pinned release tarball.
  MRUBY_VERSION = "3.3.0"

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

  MRUBY_TARBALL_NAME = "mruby-#{MRUBY_VERSION}.tar.gz".freeze
  MRUBY_UNPACKED_DIR = "mruby-#{MRUBY_VERSION}".freeze

  DEFAULT_WASI_SDK_BASE =
    "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-#{WASI_SDK_VERSION}".freeze
  DEFAULT_MRUBY_BASE = "https://github.com/mruby/mruby/archive/refs/tags"

  WASI_SDK_FINAL    = File.join(VENDOR_DIR, "wasi-sdk").freeze
  MRUBY_FINAL       = File.join(VENDOR_DIR, "mruby").freeze
  WASI_SDK_SENTINEL = "bin/clang"
  MRUBY_SENTINEL    = "Rakefile"

  WASI_TARBALL_PATH  = File.join(CACHE_DIR, WASI_SDK_TARBALL_NAME).freeze
  MRUBY_TARBALL_PATH = File.join(CACHE_DIR, MRUBY_TARBALL_NAME).freeze

  # When KOBAKO_VENDOR_BASE_URL is set, both tarballs are fetched from that
  # base URL (test fixture). The base URL must serve files named exactly
  # `WASI_SDK_TARBALL_NAME` and `MRUBY_TARBALL_NAME`.
  def self.base_url_for(default)
    override = ENV["KOBAKO_VENDOR_BASE_URL"]
    return default if override.nil? || override.empty?

    override.chomp("/")
  end

  def self.wasi_sdk_sha256
    ENV.fetch("KOBAKO_VENDOR_WASI_SDK_SHA256", "")
  end

  def self.mruby_sha256
    ENV.fetch("KOBAKO_VENDOR_MRUBY_SHA256", "")
  end

  # -----------------------------------------------------------------------
  # Helpers
  # -----------------------------------------------------------------------

  def self.download(url, dest)
    FileUtils.mkdir_p(File.dirname(dest))
    tmp = "#{dest}.part"
    URI.parse(url).open("rb") { |io| File.open(tmp, "wb") { |f| IO.copy_stream(io, f) } }
    File.rename(tmp, dest)
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
      unless actual == expected_sha
        raise "checksum mismatch for #{File.basename(path)}: " \
              "expected #{expected_sha}, got #{actual}"
      end
      File.write(sidecar, "#{actual}\n")
      return actual
    end

    if File.exist?(sidecar)
      pinned = File.read(sidecar).strip
      unless actual == pinned
        raise "checksum drift for #{File.basename(path)}: " \
              "pinned #{pinned}, got #{actual}"
      end
    else
      File.write(sidecar, "#{actual}\n")
    end

    actual
  end

  # Prepare an unpacked tree at `final_dir` from `tarball`'s `top_level_dir`.
  # If `final_dir` already exists with the sentinel inside, this is a no-op.
  def self.prepare_unpacked(tarball:, top_level_dir:, final_dir:, sentinel:)
    return if File.exist?(File.join(final_dir, sentinel))

    staging = "#{final_dir}.staging"
    FileUtils.rm_rf(staging)
    FileUtils.mkdir_p(staging)
    system("tar", "-xzf", tarball, "-C", staging, exception: true)

    src = File.join(staging, top_level_dir)
    raise "expected #{src} after extracting #{tarball}, missing" unless File.directory?(src)

    FileUtils.rm_rf(final_dir)
    FileUtils.mkdir_p(File.dirname(final_dir))
    FileUtils.mv(src, final_dir)
    FileUtils.rm_rf(staging)
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
    KobakoVendor.verify_or_pin(t.name, KobakoVendor.wasi_sdk_sha256)
  end

  file KobakoVendor::MRUBY_TARBALL_PATH do |t|
    url = "#{KobakoVendor.base_url_for(KobakoVendor::DEFAULT_MRUBY_BASE)}/" \
          "#{KobakoVendor::MRUBY_TARBALL_NAME}"
    puts "[vendor] downloading mruby #{KobakoVendor::MRUBY_VERSION} from #{url}"
    KobakoVendor.download(url, t.name)
    KobakoVendor.verify_or_pin(t.name, KobakoVendor.mruby_sha256)
  end

  desc "Download and unpack wasi-sdk #{KobakoVendor::WASI_SDK_FULL_VERSION} into vendor/wasi-sdk/"
  task setup_wasi_sdk: KobakoVendor::WASI_TARBALL_PATH do
    KobakoVendor.verify_or_pin(KobakoVendor::WASI_TARBALL_PATH, KobakoVendor.wasi_sdk_sha256)
    KobakoVendor.prepare_unpacked(
      tarball: KobakoVendor::WASI_TARBALL_PATH,
      top_level_dir: KobakoVendor::WASI_SDK_UNPACKED_DIR,
      final_dir: KobakoVendor::WASI_SDK_FINAL,
      sentinel: KobakoVendor::WASI_SDK_SENTINEL
    )
    puts "[vendor] wasi-sdk ready at #{KobakoVendor::WASI_SDK_FINAL}"
  end

  desc "Download and unpack mruby #{KobakoVendor::MRUBY_VERSION} into vendor/mruby/"
  task setup_mruby: KobakoVendor::MRUBY_TARBALL_PATH do
    KobakoVendor.verify_or_pin(KobakoVendor::MRUBY_TARBALL_PATH, KobakoVendor.mruby_sha256)
    KobakoVendor.prepare_unpacked(
      tarball: KobakoVendor::MRUBY_TARBALL_PATH,
      top_level_dir: KobakoVendor::MRUBY_UNPACKED_DIR,
      final_dir: KobakoVendor::MRUBY_FINAL,
      sentinel: KobakoVendor::MRUBY_SENTINEL
    )
    puts "[vendor] mruby ready at #{KobakoVendor::MRUBY_FINAL}"
  end

  desc "Fetch and unpack all build-time vendor toolchains (wasi-sdk + mruby)"
  task setup: %i[setup_wasi_sdk setup_mruby]

  desc "Remove unpacked vendor toolchains (keeps cached tarballs)"
  task :clean do
    FileUtils.rm_rf(KobakoVendor::WASI_SDK_FINAL)
    FileUtils.rm_rf(KobakoVendor::MRUBY_FINAL)
  end

  desc "Remove vendor/ entirely (unpacked trees and cached tarballs)"
  task :clobber do
    FileUtils.rm_rf(KobakoVendor::VENDOR_DIR)
  end
end
