# frozen_string_literal: true

# wasm/ Rust crate (kobako-wasm) tasks.
# ====================================
#
# kobako-wasm is the Guest Binary source; see SPEC.md "Wire Codec" and
# tmp/REFERENCE.md Ch.5. Two tasks here:
#
#   * `rake wasm:check` — compile-only check. Targets wasm32-wasip1 if the
#                         toolchain target is installed (cargo handles
#                         download via rustup); otherwise falls back to the
#                         host target so the task still produces a useful
#                         signal in environments without wasi-sdk.
#   * `rake wasm:test`  — runs `cargo test` on the host. wasm32 has no test
#                         runner, so codec unit tests must run on the host
#                         build of the same code.
#
# Neither task requires wasi-sdk (those bindings come in via item #10 when
# we link the actual mruby-bearing artifact). For the codec skeleton, plain
# cargo on stable Rust is enough.

require "open3"

# Hoisted out of the `namespace :wasm` block so that constant definitions
# don't trigger Lint/ConstantDefinitionInBlock and match the style used
# by tasks/vendor.rake (KobakoVendor module).
module KobakoWasm
  ROOT       = File.expand_path("..", __dir__)
  CRATE_DIR  = File.join(ROOT, "wasm", "kobako-wasm").freeze
  MANIFEST   = File.join(CRATE_DIR, "Cargo.toml").freeze
  WASM_TARGET = "wasm32-wasip1"

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
end
