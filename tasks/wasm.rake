# frozen_string_literal: true

# wasm/ sub-workspace (Guest Binary) tasks.
# =========================================
#
# The `kobako-wasm` shell crate (composing the `kobako-core` contract
# crate with mruby) is the Guest Binary source; see SPEC.md "Wire Codec".
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
#                         to `data/kobako.wasm`. Depends on `beni:build`
#                         (Stages A+B: toolchain vendoring + libmruby.a)
#                         so the full pipeline runs end-to-end from a
#                         clean clone with a single command.
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
    # `--workspace` covers both member crates (`kobako-core`,
    # `kobako-wasm`) so the wasm32 lane catches breakage in either.
    sh env, "cargo", "check", "--manifest-path", KobakoWasm::MANIFEST, "--workspace", *target_flag
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

# Stage C artifact tasks. A second `namespace :wasm` block (rake reopens
# the namespace) keeps the build/clean group separate from the check/test
# signal group above. The pure default and the two regexp variants share
# one GuestBuilder, parameterised by cargo features and output path.
namespace :wasm do
  desc "Build Guest Binary (data/kobako.wasm) from kobako-wasm crate + libmruby.a (Stage C)"
  task build: ["beni:build"] do
    KobakoWasm.ensure_cargo!
    KobakoWasm::GuestBuilder.new.build
  end

  desc "Build the regexp variant Guest Binary (data/kobako+regexp.wasm; no Unicode)"
  task "build:regexp" => ["beni:build"] do
    KobakoWasm.ensure_cargo!
    KobakoWasm::GuestBuilder.new(features: ["regexp"], output: KobakoWasm::DATA_WASM_REGEXP).build
  end

  desc "Build the regexp+unicode variant Guest Binary (data/kobako+regexp-unicode.wasm)"
  task "build:regexp_unicode" => ["beni:build"] do
    KobakoWasm.ensure_cargo!
    KobakoWasm::GuestBuilder.new(features: ["regexp-unicode"], output: KobakoWasm::DATA_WASM_REGEXP_UNICODE).build
  end

  desc "Remove every Guest Binary variant and the wasm crate target/ cache"
  task :clean do
    [KobakoWasm::DATA_WASM, KobakoWasm::DATA_WASM_REGEXP, KobakoWasm::DATA_WASM_REGEXP_UNICODE].each do |wasm|
      FileUtils.rm_f(wasm)
    end
    FileUtils.rm_rf(KobakoWasm::CRATE_TARGET_DIR)
    puts "[wasm:clean] removed the Guest Binary variants and #{KobakoWasm::CRATE_TARGET_DIR}"
  end
end
