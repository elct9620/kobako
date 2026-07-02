# frozen_string_literal: true

# crates/ sub-workspace signal tasks.
#
#   * `rake crates:test` — runs `cargo test` over the crates/ workspace
#                          (`kobako-codec`, `kobako-runtime`,
#                          `kobako-wasmtime`) — including the wire-tier
#                          codec / envelope unit tests. All members
#                          compile host-native, so no cross toolchain
#                          is involved.

require_relative "support/wasm"

namespace :crates do
  desc "cargo test the crates/ workspace (kobako-codec, kobako-runtime, kobako-wasmtime)"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run crates:test" unless KobakoWasm.cargo_available?
    manifest = File.expand_path("../crates/Cargo.toml", __dir__)
    sh "cargo", "test", "--manifest-path", manifest, "--workspace"
  end
end
