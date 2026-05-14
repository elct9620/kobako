# frozen_string_literal: true

# mruby build configuration for the kobako Guest Binary.
# =====================================================
#
# Drives mruby's build system (Stage B of the Build Pipeline) to produce
# `vendor/mruby/build/wasi/lib/libmruby.a`, cross-compiled for
# `wasm32-wasip1` against the vendored wasi-sdk toolchain.
#
# This file encodes the five customisation rules:
#
#   1. mrbgem allowlist — no I/O / network / sleep / random-seed gems leak
#      into the guest binary; only the core extension gems listed below.
#   2. CC / AR / LD pinned to vendor/wasi-sdk/bin/* (no host clang or libc).
#   3. setjmp/longjmp three-flag set, applied to every translation unit AND
#      to the link step:
#         -mllvm -wasm-enable-sjlj
#         -lsetjmp
#         -mllvm -wasm-use-legacy-eh=false
#   4. `-D` flag central management; integer width pinned to MRB_INT32 and
#      mrb_value layout pinned to MRB_WORDBOX_NO_INLINE_FLOAT.
#   5. VM dispatch mode left at mruby default (no MRB_USE_VM_SWITCH_DISPATCH).
#
# This file is `load`ed by mruby's minirake when the wrapping rake task
# (tasks/mruby.rake) sets `MRUBY_CONFIG=$PWD/build_config/wasi.rb`. The
# top-level `MRuby::CrossBuild.new("wasi") do |conf| ... end` block is the
# documented entry point of the mruby build DSL. CrossBuild (rather than
# Build) is used because mruby 4.0 requires a host-side mrbc to compile the
# mrblib; CrossBuild auto-creates a minimal host target for that purpose.

# Resolve vendor toolchain paths relative to this file. mruby's build system
# `instance_eval`s this file in the context of MRuby::RakeFile (which has no
# `__dir__`-equivalent helper), so we anchor on `__FILE__` explicitly.
# Config-time constants live in a dedicated namespace. The whole module is
# only defined on first load, so `load`-ing this file twice in the same
# process (e.g. across test runs) does not warn about constant redefinition.
unless defined?(KobakoBuildConfig)
  # Config-time constants and helpers shared across the mruby Stage B
  # build (this file) and the rake wrappers in `tasks/`. Wrapped in
  # `unless defined?` so re-loading this file (e.g. across test runs)
  # does not warn about constant redefinition.
  module KobakoBuildConfig
    CONFIG_DIR   = File.expand_path(__dir__)
    PROJECT_ROOT = File.expand_path("..", CONFIG_DIR)
    VENDOR_DIR   = (ENV["KOBAKO_VENDOR_DIR"] || File.join(PROJECT_ROOT, "vendor")).freeze
    WASI_SDK     = (ENV["WASI_SDK_PATH"] || File.join(VENDOR_DIR, "wasi-sdk")).freeze
    WASI_SYSROOT = File.join(WASI_SDK, "share", "wasi-sysroot").freeze

    # The three setjmp/longjmp flags. All three must be present at *both*
    # compile and link stages; missing any one trips wasi-libc's
    # `<setjmp.h>` build-time `#error`.
    SJLJ_FLAGS = [
      "-mllvm", "-wasm-enable-sjlj",
      "-mllvm", "-wasm-use-legacy-eh=false"
    ].freeze

    # Cross-compile target. `wasm32-wasi` is the LLVM triple (same ABI
    # as Rust's `wasm32-wasip1` target); the LLVM-triple form is what
    # clang accepts on the command line.
    WASI_TARGET = "wasm32-wasi"

    # mruby `CrossBuild` name — controls the build subdirectory layout
    # (`vendor/mruby/build/<name>/`) and the artifact path the rake
    # wrapper expects (`KobakoMruby::TARGET_NAME` in tasks/mruby.rake
    # MUST agree). Bumping requires touching both files; the constant
    # is hoisted here so paths derived from the build subdir stay in
    # sync.
    MRUBY_BUILD_NAME = "wasi"

    # Target / sysroot flags applied to every translation unit AND the link
    # step. Frozen so a stray `<<` in the build block raises instead of
    # silently mutating the shared reference.
    TARGET_FLAGS = [
      "--target=#{WASI_TARGET}",
      "--sysroot=#{WASI_SYSROOT}"
    ].freeze

    # The kobako mrbgem allowlist (rule #1, core gems).
    # Strict allowlist: anything not enumerated here MUST NOT enter the
    # guest binary. I/O, network, sleep, random-seed gems are deliberately
    # excluded to shrink the attack surface. Bumping this list is a wire- /
    # security-review-bearing change.
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
      mruby-symbol-ext
      mruby-error
      mruby-metaprog
    ].freeze

    # Third-party mrbgems vendored under `vendor/<name>/` by
    # `tasks/vendor.rake`. Same strict-allowlist contract as
    # `MRBGEM_ALLOWLIST` but a separate surface: each entry pulls in a
    # native C dependency too, so the attack surface widens beyond the
    # gem's Ruby + glue C. Adding to this list is a wire- / security-
    # review-bearing change.
    #
    #   * mruby-onig-regexp — Onigmo regex engine (mruby 4.0 ships no
    #     built-in Regexp). Onigmo is a guest-side compute capability;
    #     Regexp objects do NOT cross the host↔guest wire (no SPEC.md
    #     wire codec change). The Onigmo source bundled by the gem is
    #     frozen at 6.2.0 (2019) and carries known CVEs covering
    #     ReDoS, OOB reads, and OOB writes; upgrading requires forking
    #     the gem because the version is hard-coded in its
    #     mrbgem.rake. The wasm sandbox isolates the host from any
    #     guest-side crash, but a malicious / malformed pattern can
    #     still corrupt guest state — host-side Sandbox limits (fuel,
    #     memory) bound compute exhaustion but cannot bound engine-
    #     internal memory-safety bugs.
    THIRD_PARTY_GEM_DIRS = [
      File.join(VENDOR_DIR, "mruby-onig-regexp")
    ].freeze

    # Onigmo 6.2.0 ships pre-wasm `config.sub` / `config.guess` that
    # reject `wasm32-wasi` host triples. mruby-onig-regexp's
    # `mrbgem.rake` extracts Onigmo into
    # `build/wasi/mrbgems/mruby-onig-regexp/onigmo-6.2.0/` only when its
    # +file header+ rake task fires, but the same +file+ rule is
    # idempotent (it skips when the sentinel exists). We pre-extract the
    # tarball and overwrite the aux scripts here so the rule sees the
    # sentinel and falls through to +./configure+, which then picks up
    # the modern wasm-aware aux scripts. Hooking the rake task graph
    # directly is not viable: mruby's build system registers gem file
    # tasks later in a separate pass than the build_config DSL.
    ONIGMO_RELATIVE_BUILD_DIR = "vendor/mruby/build/#{MRUBY_BUILD_NAME}/mrbgems/mruby-onig-regexp".freeze
    ONIGMO_RELATIVE_TARBALL   = "vendor/mruby-onig-regexp/onigmo-6.2.0.tar.gz"
    ONIGMO_VERSION_DIR        = "onigmo-6.2.0"

    def self.pre_extract_and_patch_onigmo!
      build_dir = File.join(PROJECT_ROOT, ONIGMO_RELATIVE_BUILD_DIR)
      oniguruma_dir = File.join(build_dir, ONIGMO_VERSION_DIR)
      return if File.exist?(File.join(oniguruma_dir, "onigmo.h"))

      extract_onigmo_tarball(build_dir)
      overwrite_config_aux(oniguruma_dir)
    end

    def self.extract_onigmo_tarball(build_dir)
      tarball = File.join(PROJECT_ROOT, ONIGMO_RELATIVE_TARBALL)
      return unless File.exist?(tarball)

      FileUtils.mkdir_p(build_dir)
      system("tar", "-xzf", tarball, "-C", build_dir, exception: true)
    end

    def self.overwrite_config_aux(oniguruma_dir)
      aux_dir = File.join(VENDOR_DIR, "onigmo-build-aux")
      %w[config.sub config.guess].each do |name|
        src = require_aux_script(aux_dir, name)
        dst = File.join(oniguruma_dir, name)
        FileUtils.cp(src, dst)
        File.chmod(0o755, dst)
      end
    end

    # Returns the absolute path to a vendored GNU config aux script, or
    # raises with an actionable +rake+ hint when the script is missing
    # (typically the operator forgot +rake vendor:setup:onigmo_build_aux+
    # before +rake mruby:build+). Failing here means Onigmo's pre-wasm
    # +./configure+ never runs, which would otherwise blow up downstream
    # with the cryptic +"Invalid configuration 'wasm32-wasi'"+ error.
    def self.require_aux_script(aux_dir, name)
      src = File.join(aux_dir, name)
      return src if File.exist?(src)

      raise "[kobako] missing #{src} — run `rake vendor:setup:onigmo_build_aux`"
    end
  end
end

# Register the +:wasi+ toolchain encapsulating wasi-sdk absolute paths,
# the wasm32-wasi target / sysroot flags, the setjmp/longjmp three-flag
# set, the GNU archive format, and the wire-ABI +-D+ flags (rules #2-#4
# of the file header). Cross builds opt in via +conf.toolchain :wasi+;
# the host build stays on +:gcc+ so each target picks the toolchain
# that matches it.
#
# The PATH prepend lives in this block because the only consumer of
# bare-name LLVM tools is autotools child processes spawned during the
# cross build (mruby-onig-regexp's +mrbgem.rake+ +`ar x ...`+). The
# +vendor/wasi-sdk/bin+ directory ships no +gcc+ / +g+++ binary, so
# the host build's bare +gcc+ / +g+++ default (gcc.rake) is unaffected.
MRuby::Toolchain.new(:wasi) do |conf, _params|
  wasi_sdk_bin = File.join(KobakoBuildConfig::WASI_SDK, "bin")

  conf.toolchain :clang

  # ---- Tool commands pinned to wasi-sdk absolute paths -----------------
  conf.cc.command       = File.join(wasi_sdk_bin, "clang")
  conf.cxx.command      = File.join(wasi_sdk_bin, "clang++")
  conf.linker.command   = File.join(wasi_sdk_bin, "clang")
  conf.archiver.command = File.join(wasi_sdk_bin, "llvm-ar")
  # llvm-ar on macOS hosts defaults to Darwin (BSD) archive format,
  # which can fail with "section too large" when the archive contains
  # many wasm objects with long member paths (mruby + Onigmo together
  # cross that threshold). GNU format uses an extended string table.
  conf.archiver.archive_options = "--format=gnu rs %<outfile>s %<objs>s"

  # ---- Bare-tool PATH for autotools-driven mrbgems ---------------------
  ENV["PATH"] = "#{wasi_sdk_bin}:#{ENV.fetch("PATH", "")}"

  # ---- Cross-compile target / sysroot ----------------------------------
  conf.cc.flags     << KobakoBuildConfig::TARGET_FLAGS
  conf.cxx.flags    << KobakoBuildConfig::TARGET_FLAGS
  conf.linker.flags << KobakoBuildConfig::TARGET_FLAGS

  # ---- setjmp/longjmp (rule #3) ----------------------------------------
  # Apply at compile AND link stages — three-flag set is non-negotiable.
  conf.cc.flags     << KobakoBuildConfig::SJLJ_FLAGS
  conf.cxx.flags    << KobakoBuildConfig::SJLJ_FLAGS
  conf.linker.flags << KobakoBuildConfig::SJLJ_FLAGS
  conf.linker.libraries << "setjmp" # expands to `-lsetjmp` (wasi-libc libsetjmp.a)

  # ---- `-D` flags (rule #4) --------------------------------------------
  # MRB_WORDBOX_NO_INLINE_FLOAT — pin mrb_value layout to the wasm32
  # default; the host-side wire codec assumes this layout, changing it
  # breaks the ABI. MRB_INT32 pins the integer width.
  conf.cc.defines  << "MRB_WORDBOX_NO_INLINE_FLOAT"
  conf.cxx.defines << "MRB_WORDBOX_NO_INLINE_FLOAT"
  conf.cc.defines  << "MRB_INT32"
  conf.cxx.defines << "MRB_INT32"

  # Rule #5: we deliberately do NOT add `MRB_USE_VM_SWITCH_DISPATCH`.
  # mruby's default computed-goto path is rewritten by LLVM
  # IndirectBrExpandPass into a switch+br_table on the wasm32 backend —
  # the produced code is structurally equivalent to switch dispatch.
end

# Host build short-circuits mruby's auto-host-creation
# (vendor/mruby/lib/mruby/build.rb:573). Explicit +:gcc+ keeps
# +cc.command+ as bare +gcc+ — wasi-sdk/bin has no +gcc+/+g+++, so the
# +:wasi+ toolchain's PATH prepend does not shadow the system compiler.
MRuby::Build.new("host") do |conf|
  conf.toolchain :gcc
  conf.build_mrbc_exec
  conf.disable_libmruby
end

MRuby::CrossBuild.new(KobakoBuildConfig::MRUBY_BUILD_NAME) do |conf|
  conf.toolchain :wasi

  # Cross-compile signal: third-party mrbgems (mruby-onig-regexp ships
  # its own Onigmo source and runs `./configure --host=<value>` against
  # it). Without this attribute, mruby-onig-regexp falls back to
  # `build.name` ("wasi"), which autotools does not recognise as a
  # canonical triple.
  conf.host_target = KobakoBuildConfig::WASI_TARGET

  # mrbgem allowlist (rule #1) — anything not enumerated is omitted by
  # construction. Bumping the list is a security-review-bearing change.
  KobakoBuildConfig::MRBGEM_ALLOWLIST.each { |gem_name| conf.gem core: gem_name }

  # Third-party mrbgems loaded from `vendor/<name>/`. Same strict-
  # allowlist contract; see KobakoBuildConfig::THIRD_PARTY_GEM_DIRS.
  KobakoBuildConfig::THIRD_PARTY_GEM_DIRS.each { |gem_dir| conf.gem gem_dir }

  # Pre-extract Onigmo and overwrite its pre-wasm config.sub/config.guess
  # so mrbgem.rake's file rule skips its own extraction and ./configure
  # sees the wasm-aware aux scripts.
  KobakoBuildConfig.pre_extract_and_patch_onigmo!
end
