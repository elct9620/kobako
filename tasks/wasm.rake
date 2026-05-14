# frozen_string_literal: true

# wasm/ Rust crate (kobako-wasm) tasks.
# ====================================
#
# kobako-wasm is the Guest Binary source; see SPEC.md "Wire Codec".
# Tasks here:
#
#   * `rake wasm:check` — compile-only check. Targets wasm32-wasip1 if the
#                         toolchain target is installed (cargo handles
#                         download via rustup); otherwise falls back to the
#                         host target so the task still produces a useful
#                         signal in environments without wasi-sdk.
#   * `rake wasm:test`  — runs `cargo test` on the host. wasm32 has no test
#                         runner, so codec unit tests must run on the host
#                         build of the same code.
#   * `rake wasm:build` — Stage C of the Build Pipeline. Cross-compiles
#                         the kobako-wasm crate against the vendored wasi-sdk +
#                         libmruby.a and writes the resulting Guest Binary
#                         to `data/kobako.wasm`. Depends on `vendor:setup`
#                         and `mruby:build` so the full three-stage pipeline
#                         runs end-to-end from a clean clone with a single
#                         command.
#   * `rake wasm:clean` — removes the produced `data/kobako.wasm` and
#                         the wasm crate's `target/` cache directory.
#
# `wasm:check` and `wasm:test` do not require wasi-sdk; they run with plain
# host cargo so feedback is fast in lanes without a vendored toolchain.
# Only `wasm:build` walks the full Stage A → B → C chain.
#
# Stage C helpers (paths, target detection, mtime idempotency, cargo env)
# live in tasks/support/kobako_wasm.rb.

require_relative "support/kobako_wasm"

namespace :wasm do
  desc "cargo check the kobako-wasm crate (wasm32-wasip1 if available, host otherwise)"
  task :check do
    abort "cargo not on PATH; install Rust toolchain to run wasm:check" unless KobakoWasm.cargo_available?
    target_flag = (t = KobakoWasm.wasm_target_or_host) ? ["--target", t] : []
    sh "cargo", "check", "--manifest-path", KobakoWasm::MANIFEST, *target_flag
  end

  desc "cargo test the kobako-wasm crate on the host (wasm32 has no test runner)"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run wasm:test" unless KobakoWasm.cargo_available?
    sh "cargo", "test", "--manifest-path", KobakoWasm::MANIFEST
  end

  desc "Build Guest Binary (data/kobako.wasm) from kobako-wasm crate + libmruby.a (Stage C)"
  task build: ["vendor:setup", "mruby:build"] do
    abort "cargo not on PATH; install Rust toolchain to run wasm:build" unless KobakoWasm.cargo_available?
    KobakoWasm::GuestBuilder.new.build
  end

  desc "Remove data/kobako.wasm and the wasm crate target/ cache"
  task :clean do
    FileUtils.rm_f(KobakoWasm::DATA_WASM)
    FileUtils.rm_rf(KobakoWasm::CRATE_TARGET_DIR)
    puts "[wasm:clean] removed #{KobakoWasm::DATA_WASM} and #{KobakoWasm::CRATE_TARGET_DIR}"
  end
end
