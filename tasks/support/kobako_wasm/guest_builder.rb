# frozen_string_literal: true

require "fileutils"

module KobakoWasm
  # Stage C build orchestrator: cross-compiles +kobako-wasm+ for
  # +wasm32-wasip1+ against the vendored +libmruby.a+ and copies the
  # produced wasm into +data/kobako.wasm+. One instance per build run.
  #
  # The caller (+rake wasm:build+) is responsible for guarding
  # +KobakoWasm.cargo_available?+ before constructing the builder;
  # everything else — the mtime-based up-to-date check, the Stage B
  # pre-condition, the cargo env bundle, and the artefact copy — is
  # encapsulated here.
  #
  # ## Linker choice: rust-lld (not wasi-sdk clang)
  #
  # We intentionally do NOT set CARGO_TARGET_WASM32_WASIP1_LINKER.
  # Cargo's default for wasm32-wasip1 is rust's built-in rust-lld which
  # links the cdylib with `--no-entry` (WASI reactor model) without the
  # `-shared` flag. wasi-sdk clang, by contrast, drives wasm-ld with
  # `-static -shared` which enforces PIC relocations on all input objects.
  # Neither libmruby.a nor the Rust standard library wasm32-wasip1 prebuilts
  # are compiled with -fPIC, causing wasm-ld to reject them. rust-lld's
  # `--no-entry` mode does not enforce PIC, so the link succeeds cleanly.
  #
  # The CC_wasm32_wasip1 / AR_wasm32_wasip1 / WASI_SDK_PATH env vars remain
  # set for any future build.rs steps (e.g. bindgen C compilation) that need
  # the wasi-sdk toolchain; they do not affect the Rust+mruby link step.
  class GuestBuilder
    # Build +data/kobako.wasm+ if its mtime is older than any input source.
    # Short-circuits to a no-op (with a diagnostic +puts+) when the artefact
    # is already up to date. Raises with a Stage B hint when +libmruby.a+ is
    # missing, and raises again if cargo succeeds but the expected output
    # file is absent.
    def build
      if up_to_date?
        puts "[wasm:build] #{DATA_WASM} is up to date — skipping"
        return
      end

      ensure_libmruby_present
      run_cargo_release_build
      copy_wasm_into_data_dir
    end

    private

    # True when +data/kobako.wasm+ exists and is newer than every input
    # file the build would consume — lets a second +wasm:build+ skip the
    # cargo invocation when nothing under the crate has changed.
    def up_to_date?
      return false unless File.exist?(DATA_WASM)

      src_mtime = newest_source_mtime
      return false if src_mtime.nil?

      File.mtime(DATA_WASM) >= src_mtime
    end

    def newest_source_mtime
      files = Dir.glob(File.join(CRATE_SRC_DIR, "**", "*.{rs,rb,c}"))
      files << CRATE_BUILD_RS if File.exist?(CRATE_BUILD_RS)
      files << MANIFEST
      files << LIBMRUBY_PATH if File.exist?(LIBMRUBY_PATH)
      files.map { |f| File.mtime(f) }.max
    end

    # Stage B sentinel check. Stage C cannot link without +libmruby.a+,
    # so we assert it explicitly with a message that points at the rake
    # task that produces it.
    def ensure_libmruby_present
      return if File.exist?(LIBMRUBY_PATH)

      raise "[wasm:build] expected libmruby.a at #{LIBMRUBY_PATH}; " \
            "run `rake mruby:build` (Stage B) first"
    end

    def run_cargo_release_build
      args = ["cargo", "build", "--manifest-path", MANIFEST, "--release", "--target", WASM_TARGET]
      env  = cargo_build_env
      puts "[wasm:build] env=#{env.inspect}"
      puts "[wasm:build] ==> #{args.join(" ")}"
      raise "[wasm:build] cargo build failed" unless system(env, *args)
      raise "[wasm:build] cargo built but #{CRATE_WASM_OUTPUT} is missing" unless File.exist?(CRATE_WASM_OUTPUT)
    end

    def copy_wasm_into_data_dir
      FileUtils.mkdir_p(DATA_DIR)
      FileUtils.cp(CRATE_WASM_OUTPUT, DATA_WASM)
      puts "[wasm:build] Guest Binary ready at #{DATA_WASM} (#{File.size(DATA_WASM)} bytes)"
    end

    # Build the env hash threaded into +cargo build+. +MRUBY_LIB_DIR+ wires
    # the libmruby.a search path inside +build.rs+; the +CC+ / +AR+ pair is
    # kept honest for any future C compilation (bindgen, cc-rs) without
    # requiring another env-var pass.
    def cargo_build_env
      clang   = File.join(WASI_SDK_DIR, "bin", "clang")
      llvm_ar = File.join(WASI_SDK_DIR, "bin", "llvm-ar")

      {
        "CC_wasm32_wasip1" => clang,
        "AR_wasm32_wasip1" => llvm_ar,
        "WASI_SDK_PATH" => WASI_SDK_DIR,
        "MRUBY_LIB_DIR" => MRUBY_LIB_DIR
      }
    end
  end
end
