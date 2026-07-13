# frozen_string_literal: true

# crates/ sub-workspace signal tasks.
#
#   * `rake crates:test`     — runs `cargo test` over the crates/ workspace
#                              (`kobako-codec`, `kobako-runtime`,
#                              `kobako-wasmtime`) — including the wire-tier
#                              codec / envelope unit tests. All members
#                              compile host-native, so no cross toolchain
#                              is involved.
#   * `rake crates:coverage` — the same suite under `cargo llvm-cov`,
#                              printing per-file Rust line coverage.
#                              Characterization only — not in the release
#                              gate, no threshold enforced.

require_relative "support/wasm"

CRATES_MANIFEST = File.expand_path("../crates/Cargo.toml", __dir__)

namespace :crates do
  desc "cargo test the crates/ workspace (kobako-codec, kobako-runtime, kobako-wasmtime)"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run crates:test" unless KobakoWasm.cargo_available?
    sh "cargo", "test", "--manifest-path", CRATES_MANIFEST, "--workspace"
  end

  desc "Rust line coverage over the crates/ workspace (cargo llvm-cov; not in release gate)"
  task :coverage do
    KobakoWasm.ensure_llvm_cov!
    sh "cargo", "llvm-cov", "--manifest-path", CRATES_MANIFEST, "--workspace"
  end
end
