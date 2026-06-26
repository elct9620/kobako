# frozen_string_literal: true

# wasm/ sub-workspace (Guest Binary) signal tasks.
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
# Neither requires wasi-sdk; they run with plain host cargo so feedback is
# fast in lanes without a vendored toolchain. The Stage C artifact tasks
# (build / variants / clean) live in tasks/wasm/build.rake; shared helpers
# (paths, target detection, cargo env) in tasks/support/wasm.rb.

require_relative "../support/wasm"

namespace :wasm do
  desc "cargo check the wasm sub-workspace (wasm32-wasip1 if available, host otherwise)"
  task :check do
    abort "cargo not on PATH; install Rust toolchain to run wasm:check" unless KobakoWasm.cargo_available?
    # `beni-sys` requires the archive paths explicitly on cross targets
    # (it never reads the vendor tree there), so the wasm32 lane also
    # needs a built Stage B; without one, degrade to the host lane,
    # where `beni` compiles in placeholder mode and the check still
    # produces a useful signal.
    target = KobakoWasm.wasm_target_or_host
    target = nil if target && !File.exist?(KobakoWasm::LIBMRUBY_PATH)
    target_flag = target ? ["--target", target] : []
    env = target ? { "MRUBY_LIB_DIR" => KobakoWasm::MRUBY_LIB_DIR, "WASI_SDK_PATH" => KobakoWasm::WASI_SDK_DIR } : {}
    # `--workspace` covers every member crate (`kobako-core`, `kobako`,
    # `kobako-io`, `kobako-json`, `kobako-regexp`, `kobako-wasm`) at its
    # default features so the wasm32 lane catches breakage in any of them.
    sh env, "cargo", "check", "--manifest-path", KobakoWasm::MANIFEST, "--workspace", *target_flag

    # The default check builds the shell with no variant features, so the
    # capability compositions (extra gems + their `cfg` blocks) go
    # unchecked. Compile the shell under each named variant so broken
    # feature wiring fails here, not when a release first builds it.
    KobakoWasm::VARIANT_FEATURES.each do |feature|
      sh env, "cargo", "check", "--manifest-path", KobakoWasm::MANIFEST,
         "--package", "kobako-wasm", "--features", feature, *target_flag
    end
  end

  desc "cargo test the wasm sub-workspace on the host (wasm32 has no test runner)"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run wasm:test" unless KobakoWasm.cargo_available?
    # `--workspace` covers the `kobako-core` codec / envelope tests and
    # the `kobako-wasm` entry-body tests. The mruby wrapper / FFI tiers
    # are tested in the beni repository, which runs them against a real
    # host libmruby.a.
    sh "cargo", "test", "--manifest-path", KobakoWasm::MANIFEST, "--workspace"
  end
end
