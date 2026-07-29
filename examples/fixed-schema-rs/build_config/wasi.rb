# frozen_string_literal: true

# mruby build configuration for this example's Guest Binary.
# =========================================================
#
# Drives mruby's own build system to produce
# `vendor/mruby/build/wasi/lib/libmruby.a`, cross-compiled for
# `wasm32-wasip1` against the toolchain beni vendors. The Rust shell in
# `guest/shell` links that archive.
#
# Two of the settings below are the guest's, and two are the wire's:
#
#   * The mrbgem allowlist and the VM dispatch mode are yours to choose —
#     a guest that needs no Regexp simply omits the gem, and nothing on
#     the wire notices.
#   * `MRB_INT32` and `MRB_WORDBOX_NO_INLINE_FLOAT` are not. They pin the
#     integer width and the `mrb_value` layout the kobako guest crates
#     were built against; changing either makes this artifact disagree
#     with the crates it links, whatever payload schema it speaks.

unless defined?(KvBuildConfig)
  # Config-time constants. Wrapped in `unless defined?` so re-loading
  # this file in one process does not warn about redefinition.
  module KvBuildConfig
    PROJECT_ROOT = File.expand_path("..", __dir__)
    # beni exports +BENI_VENDOR_DIR+ into the mruby subprocess; the
    # fallback serves a direct load of this file.
    VENDOR_DIR = (ENV["BENI_VENDOR_DIR"] || File.join(PROJECT_ROOT, "vendor")).freeze

    # The mruby `CrossBuild` name, which is also the build subdirectory
    # (`vendor/mruby/build/<name>/`). The `target :wasi` declaration in
    # the Rakefile and the archive path Stage C exports must agree with
    # it, so it is named once here.
    MRUBY_BUILD_NAME = "wasi"

    # The gems this guest gives its scripts. An allowlist rather than a
    # denylist: anything absent cannot be reached, so the surface a guest
    # script sees is what this array says and nothing more. I/O, network,
    # sleep, and random-seed gems are left out — a capability a host
    # wants to grant arrives as a bound Service, where the host stays in
    # the loop.
    MRBGEM_ALLOWLIST = %w[
      mruby-compiler
      mruby-array-ext
      mruby-enum-ext
      mruby-hash-ext
      mruby-numeric-ext
      mruby-object-ext
      mruby-proc-ext
      mruby-range-ext
      mruby-string-ext
      mruby-sprintf
      mruby-symbol-ext
      mruby-error
      mruby-metaprog
    ].freeze
  end
end

# An explicit host build short-circuits mruby's auto-host-creation.
# `:gcc` forces a bare `gcc` so mruby's toolchain guess cannot pick
# `:clang` on macOS and resolve through PATH into the wasi-sdk clang.
MRuby::Build.new("host") do |conf|
  conf.toolchain :gcc
  conf.build_mrbc_exec
  conf.disable_libmruby
end

MRuby::CrossBuild.new(KvBuildConfig::MRUBY_BUILD_NAME) do |conf|
  # The wasi-sdk tool paths, target/sysroot flags, and the setjmp/longjmp
  # flag set live in the `:wasi` toolchain file beni stages into
  # `vendor/mruby/tasks/toolchains/` during `beni:vendor:setup`.
  conf.toolchain :wasi

  KvBuildConfig::MRBGEM_ALLOWLIST.each { |gem_name| conf.gem core: gem_name }

  # The two ABI-bearing pins — see this file's header.
  conf.cc.defines  << "MRB_WORDBOX_NO_INLINE_FLOAT"
  conf.cxx.defines << "MRB_WORDBOX_NO_INLINE_FLOAT"
  conf.cc.defines  << "MRB_INT32"
  conf.cxx.defines << "MRB_INT32"
end
