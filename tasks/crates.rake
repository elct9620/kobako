# frozen_string_literal: true

# crates/ sub-workspace (native host-side crates) signal tasks.
#
#   * `rake crates:test` — runs `cargo test` over the crates/ workspace
#                          (`kobako-runtime`, `kobako-wasmtime`). The
#                          crates are host-native, so no cross toolchain
#                          is involved; `rake compile` already builds
#                          them as the ext's path dependencies, and this
#                          task runs their unit tests.

namespace :crates do
  desc "cargo test the crates/ workspace (kobako-runtime, kobako-wasmtime)"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run crates:test" unless system("cargo --version >/dev/null 2>&1")
    manifest = File.expand_path("../crates/Cargo.toml", __dir__)
    sh "cargo", "test", "--manifest-path", manifest, "--workspace"
  end
end
