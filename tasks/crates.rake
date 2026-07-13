# frozen_string_literal: true

# crates/ sub-workspace signal tasks.
#
#   * `rake crates:test`      — runs `cargo test` over the crates/ workspace
#                               (`kobako-codec`, `kobako-runtime`,
#                               `kobako-wasmtime`) — including the wire-tier
#                               codec / envelope unit tests. All members
#                               compile host-native, so no cross toolchain
#                               is involved.
#   * `rake coverage:crates`  — the same suite under `cargo llvm-cov`. Lives
#                               in the `coverage:` namespace with the other
#                               line-coverage reports, but here beside the
#                               crates manifest and cargo guard it shares.
#                               Characterization only — not in the release
#                               gate, no threshold enforced.

require_relative "support/wasm"
require_relative "support/report"

CRATES_MANIFEST = File.expand_path("../crates/Cargo.toml", __dir__)

namespace :crates do
  desc "cargo test the crates/ workspace (kobako-codec, kobako-runtime, kobako-wasmtime)"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run crates:test" unless KobakoWasm.cargo_available?
    sh "cargo", "test", "--manifest-path", CRATES_MANIFEST, "--workspace"
  end
end

namespace :coverage do
  desc "Rust line coverage over the crates/ workspace (cargo llvm-cov; not in release gate)"
  task :crates do
    KobakoWasm.ensure_llvm_cov!
    # The host driver paths run only through the gem's ext, so a partial
    # total is E2E's tier, not a unit-test gap.
    sh "cargo", "llvm-cov", "--manifest-path", CRATES_MANIFEST, "--workspace"
    reads_as = "unit-test reach only — driver paths are E2E-exercised (rake test); " \
               "behavior coverage in rake gate:anchors:coverage"
    puts KobakoReport.footer("coverage:crates", reads_as)
  end
end
