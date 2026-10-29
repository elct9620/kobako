# frozen_string_literal: true

# crates/ sub-workspace signal tasks.
#
#   * `rake crates:test`      — runs `cargo test` over every member of the
#                               crates/ workspace (see its Cargo.toml),
#                               including the core envelope's golden
#                               vectors. All members compile host-native,
#                               so no cross toolchain is involved.
#   * `rake crates:stress`    — the stress witnesses, each in a `stress`
#                               test module and skipped by `crates:test`,
#                               in an optimized build, where a race lands
#                               often enough to be caught in seconds. Not
#                               in the release gate.
#   * `rake coverage:crates`  — the same suite under `cargo llvm-cov`. Lives
#                               in the `coverage:` namespace with the other
#                               line-coverage reports, but here beside the
#                               crates manifest and cargo guard it shares.
#                               Characterization only — not in the release
#                               gate, no threshold enforced.
#   * `rake crates:graph`     — the build graphs the published host crates
#                               promise.
#   * `rake wasm:graph`       — the same for the guest tiers, which link the
#                               mruby archive, so it lives in the `wasm:`
#                               namespace with the guest's other checks.

require_relative "support/wasm"
require_relative "support/rust_coverage"
require_relative "support/report"

CRATES_MANIFEST = File.expand_path("../crates/Cargo.toml", __dir__)

# The replaceable seams docs/guides/customization.md promises are facts
# about build graphs, which no test binary can host: a tier that only
# routes builds with no payload codec, the Rust SDK builds with no wasm
# engine, and the driver a GVL-released span calls links no magnus, so
# that span cannot reach a Ruby VALUE.
#
# A built probe compiles +--all-targets+ first: a crate whose library
# drops a dependency while its tests keep it has moved it out of the
# shipped graph without moving it out of the code. A graph is read with
# the edges a consumer installs — +[dev-dependencies]+ and proc macros
# expand nothing into it — except the magnus probe, which reads every edge.
#
# The guest tiers compile on the host and link the archive Stage B
# stages, hence +wasm:graph+'s +beni:build+ prerequisite and
# +KobakoWasm::HOST_CARGO_ENV+.
CratesGraphProbe = Struct.new(:crate, :dir, :features, :edges, :denied, :allowed, :built, keyword_init: true) do
  def label = [crate, features].compact.join(" ")

  # Whether the crate compiles with every target, run inside +dir+.
  def builds? = system(KobakoWasm::HOST_CARGO_ENV, "cargo check -p #{label} --all-targets --quiet", out: File::NULL)

  # The crates its graph resolves to that the promise leaves out, run inside +dir+.
  def strays
    tree = IO.popen(KobakoWasm::HOST_CARGO_ENV, "cargo tree -p #{label}#{" -e #{edges}" if edges}",
                    err: File::NULL, &:read)
    pulled = tree.lines.drop(1).filter_map { |line| line[/[a-z0-9-]+(?= v[0-9])/] }.uniq
    allowed ? pulled - allowed : pulled & denied
  end
end

CRATES_WORKSPACE_DIR = File.dirname(CRATES_MANIFEST)
# The payload codecs and the MessagePack library beneath them, and the
# driver and the engine beneath it. Named rather than derived so adding a
# codec or an engine is a deliberate edit here.
CODEC_CRATES = %w[kobako-codec rmp rmp-serde msgpack].freeze
ENGINE_CRATES = %w[kobako-wasmtime wasmtime wasmtime-cranelift cranelift-codegen].freeze
# What a routing-only tier may resolve to: the tiers themselves plus the
# mruby wrapper and what it is built on, which are the interpreter rather
# than a schema.
ROUTING_ONLY = %w[kobako-transport kobako-core kobako-mruby beni beni-sys bytemuck].freeze
SHIPPED_EDGES = "normal,no-proc-macro"

CRATES_GRAPH_PROBES = [
  # A routing-only tier on its default build, the graph a third party gets
  # for naming one; Cargo's default set can be added to but never taken from.
  *{ "kobako-transport" => CRATES_WORKSPACE_DIR,
     "kobako-core" => KobakoWasm::WASM_WORKSPACE_DIR,
     "kobako-mruby" => KobakoWasm::WASM_WORKSPACE_DIR }.map do |crate, dir|
    CratesGraphProbe.new(crate: crate, dir: dir, edges: SHIPPED_EDGES, allowed: ROUTING_ONLY, built: true)
  end,
  # The SDK an embedder names directly defaults to both, so each claim
  # deselects one. The codec probe keeps the engine, which its integration
  # tests load a Guest Binary with; the engine's own cases drop out
  # through `#![cfg(feature = "wasmtime")]` rather than an exclusion here.
  CratesGraphProbe.new(crate: "kobako", dir: CRATES_WORKSPACE_DIR,
                       features: "--no-default-features --features wasmtime",
                       edges: SHIPPED_EDGES, denied: CODEC_CRATES, built: true),
  CratesGraphProbe.new(crate: "kobako", dir: CRATES_WORKSPACE_DIR, features: "--no-default-features",
                       edges: SHIPPED_EDGES, denied: ENGINE_CRATES, built: true),
  CratesGraphProbe.new(crate: "kobako-wasmtime", dir: CRATES_WORKSPACE_DIR, denied: %w[magnus], built: false)
].freeze

# The probes of the guest workspace, and those of the host's.
GUEST_GRAPH_PROBES, HOST_GRAPH_PROBES =
  CRATES_GRAPH_PROBES.partition { |probe| probe.dir == KobakoWasm::WASM_WORKSPACE_DIR }.map(&:freeze)

# Report whether each of +probes+ keeps its promise, as the gate +name+.
def check_graphs(name, probes)
  violations = probes.filter_map { |probe| crates_graph_violation(probe) }
  puts KobakoReport.gate(name: name,
                         ok_summary: "#{probes.size} build graphs leave out what they promise to",
                         violations: violations, noun: "violation",
                         hint: "Each promise is written in docs/guides/customization.md.")
end

# Why +probe+ breaks its promise, or +nil+ when it keeps it.
def crates_graph_violation(probe)
  Dir.chdir(probe.dir) do
    if probe.built && !probe.builds?
      next "#{probe.label} does not build — its code still reaches what the graph leaves out"
    end

    strays = probe.strays
    "#{probe.label} reaches #{strays.join(", ")}" unless strays.empty?
  end
end

namespace :crates do
  desc "cargo test every member of the crates/ workspace"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run crates:test" unless KobakoWasm.cargo_available?
    sh "cargo", "test", "--manifest-path", CRATES_MANIFEST, "--workspace"
  end

  desc "cargo test the crates/ stress witnesses in an optimized build (not in release gate)"
  task :stress do
    abort "cargo not on PATH; install Rust toolchain to run crates:stress" unless KobakoWasm.cargo_available?
    sh "cargo", "test", "--manifest-path", CRATES_MANIFEST, "--workspace", "--release", "--", "--ignored", "stress::"
  end

  desc "Check each published host crate's build graph leaves out what its seam promises to"
  task :graph do
    check_graphs("crates:graph", HOST_GRAPH_PROBES)
  end
end

namespace :wasm do
  desc "Check each guest tier's build graph leaves out what its seam promises to"
  task graph: ["beni:build"] do
    check_graphs("wasm:graph", GUEST_GRAPH_PROBES)
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
