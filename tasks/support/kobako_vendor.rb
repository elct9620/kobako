# frozen_string_literal: true

# Vendor toolchain support module
# ===============================
#
# Pure-Ruby helpers backing +tasks/vendor.rake+. Owns pinned versions,
# tarball asset URLs, and unpacked-tree preparation. Network download
# lives in +KobakoVendor::Downloader+ and SHA256 verification lives in
# +KobakoVendor::Checksum+; the .rake wrapper is the rake DSL surface
# that glues these helpers to +file+ / +task+ declarations.
#
# Honors +KOBAKO_VENDOR_BASE_URL+ to point downloads at a local fixture
# during tests, and +KOBAKO_VENDOR_DIR+ to relocate the entire vendor
# tree (also test-only).

require_relative "kobako_vendor/downloader"
require_relative "kobako_vendor/checksum"
require_relative "kobako_vendor/tarball"

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

  WASI_SDK_TARBALL_NAME = "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}.tar.gz".freeze
  WASI_SDK_UNPACKED_DIR = "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}".freeze

  # GitHub archive tarballs for mruby are served at
  # refs/tags/{VERSION}.tar.gz (no +mruby-+ prefix in the URL filename).
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
  # savannah cgit serves raw blobs as +plain/<path>?id=<sha>+; pinning to
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

  module_function

  # When KOBAKO_VENDOR_BASE_URL is set, both tarballs are fetched from that
  # base URL (test fixture). The base URL must serve files named exactly
  # +WASI_SDK_TARBALL_NAME+ and +MRUBY_TARBALL_NAME+.
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
