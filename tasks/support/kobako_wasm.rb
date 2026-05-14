# frozen_string_literal: true

# wasm Rust crate (kobako-wasm) support module
# ============================================
#
# Pure-Ruby helpers backing +tasks/wasm.rake+. Owns crate paths,
# wasm32-wasip1 target detection, mtime-based idempotency for the Stage C
# build, and the env-var bundle threaded into +cargo build+. The .rake
# wrapper is the rake DSL surface that glues these helpers to
# +rake wasm:check+ / +rake wasm:test+ / +rake wasm:build+.

require "fileutils"
require "open3"

# Stage C build helpers for the kobako-wasm crate. See sibling
# +tasks/wasm.rake+ for the rake DSL.
module KobakoWasm
  ROOT       = File.expand_path("../..", __dir__)
  CRATE_DIR  = File.join(ROOT, "wasm", "kobako-wasm").freeze
  MANIFEST   = File.join(CRATE_DIR, "Cargo.toml").freeze
  WASM_TARGET = "wasm32-wasip1"

  # Stage C inputs / outputs.
  CRATE_SRC_DIR     = File.join(CRATE_DIR, "src").freeze
  CRATE_BUILD_RS    = File.join(CRATE_DIR, "build.rs").freeze
  CRATE_TARGET_DIR  = File.join(CRATE_DIR, "target").freeze
  CRATE_WASM_OUTPUT = File.join(CRATE_TARGET_DIR, WASM_TARGET, "release", "kobako_wasm.wasm").freeze

  DATA_DIR  = File.join(ROOT, "data").freeze
  DATA_WASM = File.join(DATA_DIR, "kobako.wasm").freeze

  # Stage B output (set by tasks/mruby.rake → KobakoMruby::LIBMRUBY_PATH).
  # We avoid a require'd cross-task constant here so this file remains
  # loadable in isolation; the path is rebuilt from the same env-aware
  # vendor base.
  VENDOR_DIR     = (ENV["KOBAKO_VENDOR_DIR"] || File.join(ROOT, "vendor")).freeze
  WASI_SDK_DIR   = (ENV["WASI_SDK_PATH"] || File.join(VENDOR_DIR, "wasi-sdk")).freeze
  MRUBY_LIB_DIR  = File.join(VENDOR_DIR, "mruby", "build", "wasi", "lib").freeze
  LIBMRUBY_PATH  = File.join(MRUBY_LIB_DIR, "libmruby.a").freeze

  def self.cargo_available?
    system("which cargo > /dev/null 2>&1")
  end

  # Returns WASM_TARGET if the toolchain has it provisioned, otherwise nil
  # so the caller falls back to the host target. Keeps the task useful in
  # CI lanes that haven't yet installed the cross target.
  def self.wasm_target_or_host
    out, status = Open3.capture2("rustc", "--print", "target-list")
    return nil unless status.success?
    return nil unless out.include?(WASM_TARGET)

    # Probe whether the target's sysroot is actually present; if absent,
    # cargo check would fail. Degrade gracefully to host instead.
    _probe, probe_status = Open3.capture2(
      "rustc", "--target", WASM_TARGET, "--print", "sysroot"
    )
    probe_status.success? ? WASM_TARGET : nil
  rescue StandardError
    nil
  end

  # ---- Stage C helpers ---------------------------------------------------

  # Returns the latest mtime among the wasm crate's Rust + Cargo sources.
  # Used to short-circuit +wasm:build+ when +data/kobako.wasm+ already
  # reflects the current source tree, honouring the idempotency rule in
  # SPEC.md "Implementation Standards" for +tasks/*.rake+.
  def self.newest_source_mtime
    files = Dir.glob(File.join(CRATE_SRC_DIR, "**", "*.{rs,rb,c}"))
    files << CRATE_BUILD_RS if File.exist?(CRATE_BUILD_RS)
    files << MANIFEST
    files << LIBMRUBY_PATH if File.exist?(LIBMRUBY_PATH)
    files.map { |f| File.mtime(f) }.max
  end

  # True when `data/kobako.wasm` already exists and is newer than every
  # input file the build would consume. Skips re-running cargo in that case.
  def self.guest_wasm_up_to_date?
    return false unless File.exist?(DATA_WASM)

    src_mtime = newest_source_mtime
    return false if src_mtime.nil?

    File.mtime(DATA_WASM) >= src_mtime
  end

  # Build the env hash passed to `cargo build` for Stage C. Exports
  # `MRUBY_LIB_DIR` so build.rs can wire the libmruby.a search path.
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
  def self.cargo_build_env
    clang   = File.join(WASI_SDK_DIR, "bin", "clang")
    llvm_ar = File.join(WASI_SDK_DIR, "bin", "llvm-ar")

    {
      # cc-rs / build.rs convention for any C compilation that cargo or a
      # downstream crate may invoke. We don't currently compile C from the
      # wasm crate, but pinning these keeps a future bindgen / cc-rs hop
      # honest without requiring another env-var pass.
      "CC_wasm32_wasip1" => clang,
      "AR_wasm32_wasip1" => llvm_ar,
      "WASI_SDK_PATH" => WASI_SDK_DIR,
      "MRUBY_LIB_DIR" => MRUBY_LIB_DIR
    }
  end

  # Orchestrate Stage C: cross-compile +kobako-wasm+ and copy the
  # produced wasm into +data/kobako.wasm+. The caller (rake +wasm:build+
  # task) is responsible for guarding +cargo_available?+ and Stage B
  # ordering; the up-to-date short-circuit and Stage B sentinel check
  # live here so the orchestration is testable as a single unit.
  def self.build_guest_binary
    if guest_wasm_up_to_date?
      puts "[wasm:build] #{DATA_WASM} is up to date — skipping"
      return
    end

    unless File.exist?(LIBMRUBY_PATH)
      raise "[wasm:build] expected libmruby.a at #{LIBMRUBY_PATH}; " \
            "run `rake mruby:build` (Stage B) first"
    end

    run_cargo_release_build
    copy_wasm_into_data_dir
  end

  # Runs the cargo release build for Stage C. Both post-condition checks
  # (cargo exit status, output artefact present) are written as parallel
  # +raise ... unless+ guards; the +Style/GuardClause+ disable preserves
  # that parallel shape against rubocop's auto-correction.
  def self.run_cargo_release_build
    args = ["cargo", "build", "--manifest-path", MANIFEST, "--release", "--target", WASM_TARGET]
    env  = cargo_build_env
    puts "[wasm:build] env=#{env.inspect}"
    puts "[wasm:build] ==> #{args.join(" ")}"
    raise "[wasm:build] cargo build failed" unless system(env, *args)

    unless File.exist?(CRATE_WASM_OUTPUT) # rubocop:disable Style/GuardClause
      raise "[wasm:build] cargo build succeeded but #{CRATE_WASM_OUTPUT} is missing"
    end
  end

  def self.copy_wasm_into_data_dir
    FileUtils.mkdir_p(DATA_DIR)
    FileUtils.cp(CRATE_WASM_OUTPUT, DATA_WASM)
    puts "[wasm:build] Guest Binary ready at #{DATA_WASM} (#{File.size(DATA_WASM)} bytes)"
  end
end
