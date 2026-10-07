# frozen_string_literal: true

# crates/ sub-workspace signal tasks.
#
#   * `rake crates:test`      — runs `cargo test` over every member of the
#                               crates/ workspace (see its Cargo.toml),
#                               including the core envelope's golden
#                               vectors. All members compile host-native,
#                               so no cross toolchain is involved.
#   * `rake coverage:crates`  — the same suite under `cargo llvm-cov`. Lives
#                               in the `coverage:` namespace with the other
#                               line-coverage reports, but here beside the
#                               crates manifest and cargo guard it shares.
#                               Characterization only — not in the release
#                               gate, no threshold enforced.

require_relative "support/wasm"
require_relative "support/rust_coverage"

CRATES_MANIFEST = File.expand_path("../crates/Cargo.toml", __dir__)

namespace :crates do
  desc "cargo test every member of the crates/ workspace"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run crates:test" unless KobakoWasm.cargo_available?
    sh "cargo", "test", "--manifest-path", CRATES_MANIFEST, "--workspace"
  end
end

namespace :coverage do
  desc "crates/ Rust line coverage, files below 100% (cargo llvm-cov; not in release gate)"
  task :crates do
    # Report only the files below full coverage: the host driver paths run
    # through the gem's ext, so a partial total is E2E's tier, not a
    # unit-test gap. Run `cargo llvm-cov` directly for the full per-file view.
    reads_as = "driver paths are E2E-exercised (rake test); behavior coverage in sumi verify"
    KobakoRustCoverage.report("coverage:crates", scope: "crates/", manifest: CRATES_MANIFEST, reads_as: reads_as)
  end
end
