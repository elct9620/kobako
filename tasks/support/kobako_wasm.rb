# frozen_string_literal: true

# wasm Rust crate (kobako-wasm) support module
# ============================================
#
# Pure-Ruby helpers backing +tasks/wasm.rake+. Owns crate paths,
# wasm32-wasip1 target detection, and the Stage C orchestrator. The
# .rake wrapper is the rake DSL surface that glues these helpers to
# +rake wasm:check+ / +rake wasm:test+ / +rake wasm:build+.

require "open3"

# Stage C build helpers for the kobako-wasm crate. See sibling
# +tasks/wasm.rake+ for the rake DSL and +KobakoWasm::GuestBuilder+ for
# the orchestrator class.
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
  # Host-target +mrbc+ produced by Stage B (+MRuby::Build.new("host")+ in
  # +build_config/wasi.rb+). +build.rs+ uses this to precompile
  # +wasm/kobako-wasm/mrblib/*.rb+ into RITE bytecode embedded in the
  # cdylib (see +src/kobako/bytecode.rs+).
  MRBC_PATH      = File.join(VENDOR_DIR, "mruby", "build", "host", "bin", "mrbc").freeze

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
end

require_relative "kobako_wasm/guest_builder"
