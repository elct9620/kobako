# frozen_string_literal: true

# Vendor toolchain support module
# ===============================
#
# Pure-Ruby helpers backing +tasks/vendor.rake+. Owns pinned versions
# and declarative +Toolchain+ instances; the rake DSL surface in the
# .rake wrapper iterates +TARBALL_TOOLCHAINS+ to wire +file+ / +task+
# declarations. Network download lives in +KobakoVendor::Downloader+,
# SHA256 verification in +KobakoVendor::Checksum+, tarball extraction
# in +KobakoVendor::Tarball+, and the per-toolchain pipeline composition
# in +KobakoVendor::Toolchain+.
#
# Honors +KOBAKO_VENDOR_BASE_URL+ to point downloads at a local fixture
# during tests, and +KOBAKO_VENDOR_DIR+ to relocate the entire vendor
# tree (also test-only).
#
# Extending: a new tarball-based vendor artifact is added by appending
# one +Toolchain.new(...)+ to +TARBALL_TOOLCHAINS+ and (if its hash
# pinning is enforced via CI env var) adding a +KOBAKO_VENDOR_<KEY>_SHA256+
# entry to the deployment env. Artifacts whose pipeline does not match
# the tarball shape (plain file fetch, multi-file copy) stay declared
# by hand in +vendor.rake+.

require_relative "kobako_vendor/downloader"
require_relative "kobako_vendor/checksum"
require_relative "kobako_vendor/tarball"
require_relative "kobako_vendor/toolchain"

# Vendor toolchain façade.
module KobakoVendor
  ROOT       = File.expand_path("../..", __dir__)
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
  # Onigmo C source as +onigmo-6.2.0.tar.gz+, so no second tarball is
  # required.
  MRUBY_ONIG_REGEXP_COMMIT = "c97d7c1e7073bc5558986da4e2d07171f0761cc8"

  # GNU config aux scripts (config.sub / config.guess). Onigmo 6.2.0
  # ships pre-wasm copies that reject +wasm32-wasi+ host triples. We
  # vendor a pinned commit from GNU savannah's config.git (same source
  # CRuby's wasm build uses, see ruby/ruby +wasm/README.md+) and copy
  # them over Onigmo's shipped scripts in +build_config/wasi.rb+ before
  # +./configure+ runs.
  CONFIG_AUX_COMMIT = "a2287c3041a3f2a204eb942e09c015eab00dc7dd"

  # ---- Platform detection (wasi-sdk only; mruby tarball is host-agnostic).
  # +x86_64-linux+ is both the most common host triple and the safest
  # fallback for unrecognised ones, so we collapse both cases into the
  # +else+ branch rather than carrying an explicit +when+ that would
  # duplicate the default.
  WASI_SDK_PLATFORM =
    case RUBY_PLATFORM
    when /arm64-darwin|aarch64-darwin/ then "arm64-macos"
    when /x86_64-darwin/               then "x86_64-macos"
    when /aarch64-linux|arm64-linux/   then "arm64-linux"
    else "x86_64-linux"
    end

  # savannah cgit serves raw blobs as +plain/<path>?id=<sha>+; pinning to
  # a commit makes the response byte-stable (TOFU sidecar in CACHE_DIR
  # detects any drift).
  CONFIG_AUX_FILES = %w[config.sub config.guess].freeze
  CONFIG_AUX_FINAL = File.join(VENDOR_DIR, "onigmo-build-aux").freeze

  WASI_SDK = Toolchain.new(
    name: "wasi-sdk",
    version_label: "#{WASI_SDK_FULL_VERSION} (#{WASI_SDK_PLATFORM})",
    base_url: "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-#{WASI_SDK_VERSION}",
    tarball_name: "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}.tar.gz",
    unpacked_top_dir: "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}",
    final_dir: File.join(VENDOR_DIR, "wasi-sdk"),
    sentinel: "bin/clang",
    sha_key: :WASI_SDK
  )

  MRUBY = Toolchain.new(
    name: "mruby",
    version_label: MRUBY_VERSION,
    base_url: "https://github.com/mruby/mruby/archive/refs/tags",
    tarball_name: "#{MRUBY_VERSION}.tar.gz",
    unpacked_top_dir: "mruby-#{MRUBY_VERSION}",
    final_dir: File.join(VENDOR_DIR, "mruby"),
    sentinel: "Rakefile",
    sha_key: :MRUBY
  )

  MRUBY_ONIG_REGEXP = Toolchain.new(
    name: "mruby-onig-regexp",
    version_label: MRUBY_ONIG_REGEXP_COMMIT[0, 8],
    base_url: "https://github.com/mattn/mruby-onig-regexp/archive",
    tarball_name: "#{MRUBY_ONIG_REGEXP_COMMIT}.tar.gz",
    unpacked_top_dir: "mruby-onig-regexp-#{MRUBY_ONIG_REGEXP_COMMIT}",
    final_dir: File.join(VENDOR_DIR, "mruby-onig-regexp"),
    sentinel: "mrbgem.rake",
    sha_key: :MRUBY_ONIG_REGEXP
  )

  TARBALL_TOOLCHAINS = [WASI_SDK, MRUBY, MRUBY_ONIG_REGEXP].freeze

  module_function

  # When KOBAKO_VENDOR_BASE_URL is set, both tarballs are fetched from that
  # base URL (test fixture). The base URL must serve files named exactly
  # +tarball_name+ for each toolchain.
  def base_url_for(default)
    override = ENV.fetch("KOBAKO_VENDOR_BASE_URL", nil)
    return default if override.nil? || override.empty?

    override.chomp("/")
  end

  # Expected SHA256 for a vendored tarball, sourced from
  # +KOBAKO_VENDOR_<KEY>_SHA256+ env vars (empty string falls back to TOFU
  # sidecar pinning in +Checksum#verify_or_pin+). +key+ is the artifact
  # slug in upper snake case, e.g. +:WASI_SDK+, +:MRUBY+,
  # +:MRUBY_ONIG_REGEXP+.
  def expected_sha256(key)
    ENV.fetch("KOBAKO_VENDOR_#{key}_SHA256", "")
  end
end
