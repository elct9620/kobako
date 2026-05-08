# frozen_string_literal: true

# mruby build configuration for the kobako Guest Binary.
# =====================================================
#
# Drives mruby's build system (Stage B in tmp/REFERENCE.md Ch.5 §Build Pipeline)
# to produce `vendor/mruby/build/wasi/lib/libmruby.a`, cross-compiled for
# `wasm32-wasip1` against the vendored wasi-sdk toolchain.
#
# This file encodes the five customisation rules from REFERENCE.md Ch.5
# §mruby 客製化五條 verbatim:
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
# top-level `MRuby::Build.new("wasi") do |conf| ... end` block is the
# documented entry point of the mruby build DSL.

# Resolve vendor toolchain paths relative to this file. mruby's build system
# `instance_eval`s this file in the context of MRuby::RakeFile (which has no
# `__dir__`-equivalent helper), so we anchor on `__FILE__` explicitly.
# Config-time constants live in a dedicated namespace. The whole module is
# only defined on first load, so `load`-ing this file twice in the same
# process (e.g. across test runs) does not warn about constant redefinition.
unless defined?(KobakoBuildConfig)
  module KobakoBuildConfig
    CONFIG_DIR   = File.expand_path(__dir__)
    PROJECT_ROOT = File.expand_path("..", CONFIG_DIR)
    VENDOR_DIR   = (ENV["KOBAKO_VENDOR_DIR"] || File.join(PROJECT_ROOT, "vendor")).freeze
    WASI_SDK     = (ENV["WASI_SDK_PATH"] || File.join(VENDOR_DIR, "wasi-sdk")).freeze
    WASI_SYSROOT = File.join(WASI_SDK, "share", "wasi-sysroot").freeze

    # The three setjmp/longjmp flags from REFERENCE Ch.5 §setjmp/longjmp 啟用.
    # All three must be present at *both* compile and link stages; missing
    # any one trips wasi-libc's `<setjmp.h>` build-time `#error`.
    SJLJ_FLAGS = [
      "-mllvm", "-wasm-enable-sjlj",
      "-mllvm", "-wasm-use-legacy-eh=false"
    ].freeze

    # Cross-compile target. REFERENCE Ch.5 documents `wasm32-wasi` as the
    # LLVM triple (same ABI as Rust's `wasm32-wasip1` target); the LLVM-
    # triple form is what clang accepts on the command line.
    WASI_TARGET = "wasm32-wasi"

    # The kobako mrbgem allowlist (REFERENCE Ch.5 §mruby 客製化五條 rule #1).
    # Strict allowlist: anything not enumerated here MUST NOT enter the
    # guest binary. I/O, network, sleep, random-seed gems are deliberately
    # excluded to shrink the attack surface. Bumping this list is a wire- /
    # security-review-bearing change.
    MRBGEM_ALLOWLIST = %w[
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
  end
end

MRuby::Build.new("wasi") do |conf|
  # ---- Toolchain (rule #2: CC / AR / LD all pinned to vendor/wasi-sdk) ---
  conf.toolchain :clang

  conf.cc.command       = File.join(KobakoBuildConfig::WASI_SDK, "bin", "clang")
  conf.cxx.command      = File.join(KobakoBuildConfig::WASI_SDK, "bin", "clang++")
  conf.linker.command   = File.join(KobakoBuildConfig::WASI_SDK, "bin", "clang")
  conf.archiver.command = File.join(KobakoBuildConfig::WASI_SDK, "bin", "llvm-ar")

  # ---- Cross-compile target ---------------------------------------------
  target_flags = [
    "--target=#{KobakoBuildConfig::WASI_TARGET}",
    "--sysroot=#{KobakoBuildConfig::WASI_SYSROOT}"
  ]

  conf.cc.flags     << target_flags
  conf.cxx.flags    << target_flags
  conf.linker.flags << target_flags

  # ---- setjmp/longjmp (rule #3) -----------------------------------------
  # Apply at compile AND link stages — three-flag set is non-negotiable.
  conf.cc.flags     << KobakoBuildConfig::SJLJ_FLAGS
  conf.cxx.flags    << KobakoBuildConfig::SJLJ_FLAGS
  conf.linker.flags << KobakoBuildConfig::SJLJ_FLAGS
  conf.linker.libraries << "setjmp" # expands to `-lsetjmp` (wasi-libc libsetjmp.a)

  # ---- `-D` flags (rule #4) ---------------------------------------------
  # MRB_WORDBOX_NO_INLINE_FLOAT — pin mrb_value layout to the wasm32 default
  # documented in REFERENCE Ch.5 §mrb_value layout. This is the layout the
  # host-side wire codec assumes; changing it breaks the ABI.
  conf.cc.defines  << "MRB_WORDBOX_NO_INLINE_FLOAT"
  conf.cxx.defines << "MRB_WORDBOX_NO_INLINE_FLOAT"

  # MRB_INT32 — REFERENCE Ch.5 §mruby 客製化五條 整數寬度. Pinned because
  # MRB_INT64 would force 64-bit int wire alignment work.
  conf.cc.defines  << "MRB_INT32"
  conf.cxx.defines << "MRB_INT32"

  # Rule #5: we deliberately do NOT add `MRB_USE_VM_SWITCH_DISPATCH`.
  # mruby's default computed-goto path is rewritten by LLVM
  # IndirectBrExpandPass into a switch+br_table on the wasm32 backend — the
  # produced code is structurally equivalent to switch dispatch.

  # ---- mrbgem allowlist (rule #1) ---------------------------------------
  # Pull each allowed gem from mruby's bundled gembox source tree. Anything
  # not listed here is omitted by construction.
  KobakoBuildConfig::MRBGEM_ALLOWLIST.each do |gem_name|
    conf.gem core: gem_name
  end
end
