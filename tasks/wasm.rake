# frozen_string_literal: true

# wasm/ Rust crate (kobako-wasm) tasks.
# ====================================
#
# kobako-wasm is the Guest Binary source; see SPEC.md "Wire Codec" and
# tmp/REFERENCE.md Ch.5. Tasks here:
#
#   * `rake wasm:check` — compile-only check. Targets wasm32-wasip1 if the
#                         toolchain target is installed (cargo handles
#                         download via rustup); otherwise falls back to the
#                         host target so the task still produces a useful
#                         signal in environments without wasi-sdk.
#   * `rake wasm:test`  — runs `cargo test` on the host. wasm32 has no test
#                         runner, so codec unit tests must run on the host
#                         build of the same code.
#   * `rake wasm:guest` — Stage C of the Build Pipeline (REFERENCE Ch.5
#                         §Build Pipeline 三段). Cross-compiles the kobako-
#                         wasm crate against the vendored wasi-sdk +
#                         libmruby.a and writes the resulting Guest Binary
#                         to `data/kobako.wasm`. Depends on `vendor:setup`
#                         and `mruby:build` so the full three-stage pipeline
#                         runs end-to-end from a clean clone with a single
#                         command.
#   * `rake wasm:guest:clean` — removes the produced `data/kobako.wasm` and
#                         the wasm crate's `target/` cache directory.
#
# `wasm:check` and `wasm:test` do not require wasi-sdk; they run with plain
# host cargo so feedback is fast in lanes without a vendored toolchain.
# Only `wasm:guest` walks the full Stage A → B → C chain.

require "fileutils"
require "open3"

# Hoisted out of the `namespace :wasm` block so that constant definitions
# don't trigger Lint/ConstantDefinitionInBlock and match the style used
# by tasks/vendor.rake (KobakoVendor module).
module KobakoWasm
  ROOT       = File.expand_path("..", __dir__)
  CRATE_DIR  = File.join(ROOT, "wasm", "kobako-wasm").freeze
  MANIFEST   = File.join(CRATE_DIR, "Cargo.toml").freeze
  WASM_TARGET = "wasm32-wasip1"

  # Stage C inputs / outputs (REFERENCE Ch.5 §Build Pipeline 三段).
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

  module_function

  def cargo_available?
    system("which cargo > /dev/null 2>&1")
  end

  # Returns WASM_TARGET if the toolchain has it provisioned, otherwise nil
  # so the caller falls back to the host target. Keeps the task useful in
  # CI lanes that haven't yet installed the cross target.
  def wasm_target_or_host
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
  # Used to short-circuit `wasm:guest` when `data/kobako.wasm` already
  # reflects the current source tree (idempotency contract from item #11).
  def newest_source_mtime
    files = Dir.glob(File.join(CRATE_SRC_DIR, "**", "*.{rs,rb}"))
    files << CRATE_BUILD_RS if File.exist?(CRATE_BUILD_RS)
    files << MANIFEST
    files << LIBMRUBY_PATH if File.exist?(LIBMRUBY_PATH)
    files.map { |f| File.mtime(f) }.max
  end

  # True when `data/kobako.wasm` already exists and is newer than every
  # input file the build would consume. Skips re-running cargo in that case.
  def guest_wasm_up_to_date?
    return false unless File.exist?(DATA_WASM)

    src_mtime = newest_source_mtime
    return false if src_mtime.nil?

    File.mtime(DATA_WASM) >= src_mtime
  end

  # Build the env hash passed to `cargo build` for Stage C. Pins CC / AR to
  # wasi-sdk's clang + llvm-ar (REFERENCE Ch.5 rule #2 — the wasm crate
  # links via the same toolchain mruby was compiled with) and exports
  # `MRUBY_LIB_DIR` so build.rs can wire the libmruby.a search path.
  def cargo_build_env
    clang   = File.join(WASI_SDK_DIR, "bin", "clang")
    llvm_ar = File.join(WASI_SDK_DIR, "bin", "llvm-ar")

    {
      # Make rustc's wasm32-wasip1 link step go through wasi-sdk's clang.
      # `CARGO_TARGET_<TARGET>_LINKER` is cargo's documented per-target
      # linker override (https://doc.rust-lang.org/cargo/reference/config.html
      # #target).
      "CARGO_TARGET_WASM32_WASIP1_LINKER" => clang,
      # cc-rs / build.rs convention for any C compilation that cargo or a
      # downstream crate may invoke. We don't currently compile C from the
      # wasm crate, but pinning these keeps a future bindgen / cc-rs hop
      # honest without requiring another env-var pass.
      "CC_wasm32_wasip1" => clang,
      "AR_wasm32_wasip1" => llvm_ar,
      "WASI_SDK_PATH"    => WASI_SDK_DIR,
      "MRUBY_LIB_DIR"    => MRUBY_LIB_DIR
    }
  end
end

namespace :wasm do
  desc "cargo check the kobako-wasm crate (wasm32-wasip1 if available, host otherwise)"
  task :check do
    abort "cargo not on PATH; install Rust toolchain to run wasm:check" unless KobakoWasm.cargo_available?

    target = KobakoWasm.wasm_target_or_host
    args = ["cargo", "check", "--manifest-path", KobakoWasm::MANIFEST]
    args.push("--target", target) if target
    puts "==> #{args.join(" ")}"
    abort "wasm:check failed" unless system(*args)
  end

  desc "cargo test the kobako-wasm crate on the host (wasm32 has no test runner)"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run wasm:test" unless KobakoWasm.cargo_available?

    args = ["cargo", "test", "--manifest-path", KobakoWasm::MANIFEST]
    puts "==> #{args.join(" ")}"
    abort "wasm:test failed" unless system(*args)
  end

  desc "Build Guest Binary (data/kobako.wasm) from kobako-wasm crate + libmruby.a (Stage C)"
  task guest: ["vendor:setup", "mruby:build"] do
    abort "cargo not on PATH; install Rust toolchain to run wasm:guest" unless KobakoWasm.cargo_available?

    if KobakoWasm.guest_wasm_up_to_date?
      puts "[wasm:guest] #{KobakoWasm::DATA_WASM} is up to date — skipping"
      next
    end

    unless File.exist?(KobakoWasm::LIBMRUBY_PATH)
      raise "[wasm:guest] expected libmruby.a at #{KobakoWasm::LIBMRUBY_PATH}; " \
            "run `rake mruby:build` (Stage B) first"
    end

    args = [
      "cargo", "build",
      "--manifest-path", KobakoWasm::MANIFEST,
      "--release",
      "--target", KobakoWasm::WASM_TARGET
    ]
    env = KobakoWasm.cargo_build_env
    puts "[wasm:guest] env=#{env.inspect}"
    puts "[wasm:guest] ==> #{args.join(" ")}"
    raise "[wasm:guest] cargo build failed" unless system(env, *args)

    unless File.exist?(KobakoWasm::CRATE_WASM_OUTPUT)
      raise "[wasm:guest] cargo build succeeded but #{KobakoWasm::CRATE_WASM_OUTPUT} is missing"
    end

    FileUtils.mkdir_p(KobakoWasm::DATA_DIR)
    FileUtils.cp(KobakoWasm::CRATE_WASM_OUTPUT, KobakoWasm::DATA_WASM)
    puts "[wasm:guest] Guest Binary ready at #{KobakoWasm::DATA_WASM} " \
         "(#{File.size(KobakoWasm::DATA_WASM)} bytes)"
  end

  namespace :guest do
    desc "Remove data/kobako.wasm and the wasm crate target/ cache"
    task :clean do
      FileUtils.rm_f(KobakoWasm::DATA_WASM)
      FileUtils.rm_rf(KobakoWasm::CRATE_TARGET_DIR)
      puts "[wasm:guest:clean] removed #{KobakoWasm::DATA_WASM} and " \
           "#{KobakoWasm::CRATE_TARGET_DIR}"
    end
  end
end
