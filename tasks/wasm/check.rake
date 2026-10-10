# frozen_string_literal: true

# wasm/ sub-workspace (Guest Binary) signal tasks.
#
#   * `rake wasm:test`  — runs `cargo test` on the host. wasm32 has no test
#                         runner, so the guest crates' unit tests run on the
#                         host build of the same code.
#   * `rake coverage:wasm` — the host test run under `cargo llvm-cov`,
#                         printing per-file Rust line coverage of the guest
#                         crates. Lives in the `coverage:` namespace with the
#                         other line-coverage reports, but here beside the
#                         wasm manifest and cargo guard it shares.
#                         Characterization only — not in the release gate.
#
# Both compile `beni-sys`, which links the host archive, so each depends
# on Stage B (`beni:build`). The Stage C artifact tasks
# (build / variants / clean) live in tasks/wasm/build.rake; shared helpers
# (paths, cargo env) in tasks/support/wasm.rb.

require_relative "../support/wasm"
require_relative "../support/rust_coverage"

namespace :wasm do
  desc "cargo test the wasm sub-workspace on the host (wasm32 has no test runner)"
  task test: ["beni:build"] do
    abort "cargo not on PATH; install Rust toolchain to run wasm:test" unless KobakoWasm.cargo_available?
    # `--workspace` covers the `kobako-core` ABI / frames tests and the
    # `kobako-wasm` entry-body tests; the wire-tier codec / envelope
    # tests live in `crates/kobako-codec` and run under `crates:test`.
    # The mruby wrapper / FFI tiers are tested in the beni repository.
    sh KobakoWasm::HOST_CARGO_ENV, "cargo", "test", "--manifest-path", KobakoWasm::MANIFEST, "--workspace"
  end
end

namespace :coverage do
  desc "wasm sub-workspace Rust line coverage on the host, files below 100% (cargo llvm-cov; not in release gate)"
  task wasm: ["beni:build"] do
    # Report only the files below full coverage. The host-native test run
    # measures unit-test reach, not the wasm32 artifact — guest behavior
    # runs through the real artifact under E2E. Run `cargo llvm-cov`
    # directly for the full per-file view.
    reads_as = "wasm32 behavior is E2E-exercised via data/kobako.wasm; behavior coverage in sumi verify"
    KobakoRustCoverage.report("coverage:wasm", scope: "guest crates", manifest: KobakoWasm::MANIFEST,
                                               reads_as: reads_as, env: KobakoWasm::HOST_CARGO_ENV)
  end
end
