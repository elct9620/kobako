# frozen_string_literal: true

# wasm Rust crate (kobako-wasm) support module
# ============================================
#
# Pure-Ruby helpers backing +tasks/wasm/+. Owns crate paths,
# wasm32-wasip1 target detection, and the Stage C orchestrator. The
# .rake wrapper is the rake DSL surface that glues these helpers to
# +rake wasm:check+ / +rake wasm:test+ / +rake wasm:build+.

require "open3"

# Stage C build helpers for the kobako-wasm crate. See
# +tasks/wasm/+ for the rake DSL and +KobakoWasm::GuestBuilder+ for
# the orchestrator class.
module KobakoWasm
  ROOT = File.expand_path("../..", __dir__)
  # `wasm/` is a cargo sub-workspace whose members share a single
  # `target/` directory at the workspace root. `kobako-wasm` is the
  # cdylib-bearing shell; its sibling `kobako-core` is a path
  # dependency with no separate artifact, and the mruby wrapper comes
  # from the published `beni` crate.
  WASM_WORKSPACE_DIR = File.join(ROOT, "wasm").freeze
  CRATE_DIR  = File.join(WASM_WORKSPACE_DIR, "kobako-wasm").freeze
  MANIFEST   = File.join(CRATE_DIR, "Cargo.toml").freeze
  WASM_TARGET = "wasm32-wasip1"

  # Stage C output paths. Cargo derives the workspace-shared target
  # directory from the workspace root rather than the cdylib member.
  CRATE_TARGET_DIR  = File.join(WASM_WORKSPACE_DIR, "target").freeze
  CRATE_WASM_OUTPUT = File.join(CRATE_TARGET_DIR, WASM_TARGET, "release", "kobako_wasm.wasm").freeze

  DATA_DIR  = File.join(ROOT, "data").freeze
  DATA_WASM = File.join(DATA_DIR, "kobako.wasm").freeze

  # Regexp-capability Guest Binary variants. `+` separates the base
  # binary from an appended capability; the capability token itself uses
  # `-` (`regexp`, `regexp-unicode`). Built by `wasm:build:regexp` /
  # `wasm:build:regexp_unicode` and shipped as downloadable Release
  # assets — never bundled into the gem.
  DATA_WASM_REGEXP         = File.join(DATA_DIR, "kobako+regexp.wasm").freeze
  DATA_WASM_REGEXP_UNICODE = File.join(DATA_DIR, "kobako+regexp-unicode.wasm").freeze

  # Stage B output (produced by `rake beni:build` against
  # build_config/wasi.rb). The vendor base mirrors the `Beni::Tasks`
  # default (`BENI_VENDOR_DIR` or `vendor/` at the project root) so the
  # Stage C exports name the same tree beni populated.
  VENDOR_DIR     = (ENV["BENI_VENDOR_DIR"] || File.join(ROOT, "vendor")).freeze
  WASI_SDK_DIR   = (ENV["WASI_SDK_PATH"] || File.join(VENDOR_DIR, "wasi-sdk")).freeze
  MRUBY_LIB_DIR  = File.join(VENDOR_DIR, "mruby", "build", "wasi", "lib").freeze
  LIBMRUBY_PATH  = File.join(MRUBY_LIB_DIR, "libmruby.a").freeze

  def self.cargo_available?
    system("which cargo > /dev/null 2>&1")
  end

  # Shared guard for the Stage C build tasks: abort with an install hint
  # when cargo is absent, so each task body stays a single build call.
  def self.ensure_cargo!
    abort "cargo not on PATH; install the Rust toolchain to build the Guest Binary" unless cargo_available?
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

require_relative "kobako_wasm/baker"
require_relative "kobako_wasm/guest_builder"
