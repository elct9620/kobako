# frozen_string_literal: true

# mruby static-library build task (Stage B of the Build Pipeline).
#
# Drives mruby's bundled `minirake` against `build_config/wasi.rb`, producing
# the cross-compiled `libmruby.a` that the wasm crate (Stage C) links into
# the guest binary. This task is the single, idempotent entry point:
#
#   $ rake mruby:build      # produces vendor/mruby/build/wasi/lib/libmruby.a
#   $ rake mruby:clean      # removes mruby's build/wasi/ tree
#
# Depends on `vendor:setup` (tasks/vendor.rake), so the wasi-sdk + mruby
# tarballs are present before mruby's minirake fires its first compile.
# Idempotency: the underlying minirake is itself a make-style incremental
# build; on top of that, this task short-circuits when the libmruby.a
# sentinel already exists, so a second `rake mruby:build` invocation is a
# no-op without even invoking minirake.

require "fileutils"
require "rbconfig"

# Hoisted out of `namespace :mruby` to keep constant lookups simple from the
# test suite (mirrors the pattern in tasks/vendor.rake).
module KobakoMruby
  ROOT          = File.expand_path("..", __dir__)
  VENDOR_DIR    = (ENV["KOBAKO_VENDOR_DIR"] || File.join(ROOT, "vendor")).freeze
  MRUBY_DIR     = File.join(VENDOR_DIR, "mruby").freeze
  BUILD_CONFIG  = File.join(ROOT, "build_config", "wasi.rb").freeze

  # mruby places artefacts under `build/<target-name>/lib/libmruby.a`, where
  # `<target-name>` matches the `MRuby::Build.new(<name>)` argument in
  # `build_config/wasi.rb` (here: "wasi").
  TARGET_NAME   = "wasi"
  LIBMRUBY_PATH = File.join(MRUBY_DIR, "build", TARGET_NAME, "lib", "libmruby.a").freeze

  def self.minirake
    # mruby ships a vendored copy of `minirake` at the top of its tree.
    File.join(MRUBY_DIR, "minirake")
  end

  # Curated symlink farm exposing only the three LLVM binutils that
  # autotools-driven mrbgems need to find on PATH. Prepending the
  # *whole* `vendor/wasi-sdk/bin` would also shadow `clang`, which
  # mruby's auto-created host build (spawned by MRuby::CrossBuild for
  # mrbc / presym scanning) resolves through PATH on macOS — picking
  # up wasi-sdk's clang there compiles `error.c` against the
  # wasm32-wasi sysroot's `setjmp.h`, whose `#error` gate fires when
  # the cross-build sjlj flags are absent (and the host build has no
  # business carrying those flags).
  WASI_TOOL_SHIM_DIR = File.join(VENDOR_DIR, "wasi-tool-shims").freeze
  WASI_TOOL_SHIM_NAMES = %w[ar ranlib nm].freeze

  # Build (idempotently) the shim farm at `vendor/wasi-tool-shims/`
  # exposing `vendor/wasi-sdk/bin/llvm-{ar,ranlib,nm}` as bare
  # `ar` / `ranlib` / `nm`. The `vendor:setup` rake task — a hard
  # dependency of `mruby:build` — places wasi-sdk under
  # `vendor/wasi-sdk/` before this runs, so the symlink targets exist.
  def self.ensure_wasi_tool_shims
    wasi_sdk_bin = File.join(VENDOR_DIR, "wasi-sdk", "bin")
    FileUtils.mkdir_p(WASI_TOOL_SHIM_DIR)
    WASI_TOOL_SHIM_NAMES.each do |name|
      FileUtils.ln_sf(File.join(wasi_sdk_bin, "llvm-#{name}"),
                      File.join(WASI_TOOL_SHIM_DIR, name))
    end
    WASI_TOOL_SHIM_DIR
  end

  # Run mruby's minirake with our build config wired in via MRUBY_CONFIG.
  # The mruby build system reads MRUBY_CONFIG (absolute path or basename of
  # a file under build_config/) to choose its top-level Build definition.
  #
  # PATH is prepended with the `wasi-tool-shims` directory (NOT the full
  # `vendor/wasi-sdk/bin`) so bare `ar` / `ranlib` / `nm` used by
  # mruby-onig-regexp's libtool-built Onigmo resolve to the LLVM variants
  # without also redirecting `clang` away from the system compiler. See
  # `WASI_TOOL_SHIM_DIR` above for the host-clang regression this guards.
  def self.invoke_minirake(*args)
    shim_dir = ensure_wasi_tool_shims
    env = {
      "MRUBY_CONFIG" => BUILD_CONFIG,
      "PATH" => "#{shim_dir}:#{ENV.fetch("PATH", "")}"
    }
    cmd = [RbConfig.ruby, minirake, *args]
    puts "[mruby] cd #{MRUBY_DIR} && MRUBY_CONFIG=#{BUILD_CONFIG} #{cmd.join(" ")}"
    system(env, *cmd, chdir: MRUBY_DIR, exception: true)
  end
end

namespace :mruby do
  desc "Build vendored mruby for wasm32-wasip1 (produces #{KobakoMruby::LIBMRUBY_PATH})"
  task build: ["vendor:setup"] do
    if File.exist?(KobakoMruby::LIBMRUBY_PATH)
      puts "[mruby] libmruby.a already present at #{KobakoMruby::LIBMRUBY_PATH} — skipping"
      next
    end

    KobakoMruby.invoke_minirake

    unless File.exist?(KobakoMruby::LIBMRUBY_PATH)
      raise "[mruby] build completed but #{KobakoMruby::LIBMRUBY_PATH} is missing"
    end

    puts "[mruby] libmruby.a ready at #{KobakoMruby::LIBMRUBY_PATH}"
  end

  desc "Remove mruby's build/wasi/ tree (keeps vendored mruby source)"
  task :clean do
    build_dir = File.join(KobakoMruby::MRUBY_DIR, "build", KobakoMruby::TARGET_NAME)
    FileUtils.rm_rf(build_dir)
    puts "[mruby] removed #{build_dir}"
  end
end
