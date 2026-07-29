# frozen_string_literal: true

# Engine-replaceability gate, the sibling of `gate:payload:optional`:
# `docs/architecture.md` marks the wasm engine as something a host brings
# itself, and `Sandbox::with_runtime` takes any `Runtime`. Without this
# check that stays a claim about the code rather than a fact about the
# dependency graph — the SDK could grow a `kobako_wasmtime::` reference on
# a path the default build never questions, and only a host assembling its
# own engine would find out.
#
# The probe runs `--all-targets`: an SDK whose library stands engine-free
# while its tests do not has moved the engine out of the shipped graph
# without moving it out of the code. The cases that legitimately need one
# — they load a real Guest Binary — carry `#![cfg(feature = "wasmtime")]`
# and drop out of this build rather than being excluded here.

# The crates an engine-free graph must not contain at all: the driver and
# the engine beneath it. Named rather than derived so adding an engine is
# a deliberate edit here.
ENGINE_CRATES = %w[kobako-wasmtime wasmtime wasmtime-cranelift cranelift-codegen].freeze
ENGINE_DESELECTED = "--no-default-features"
ENGINE_ABSENT_TIERS = { "kobako" => File.expand_path("../../crates", __dir__) }.freeze

# Report why +crate+'s engine-free build still reaches an engine, or +nil+
# when it reaches none.
def engine_absent_violation(crate, dir)
  Dir.chdir(dir) do
    unless system("cargo check -p #{crate} #{ENGINE_DESELECTED} --all-targets --quiet",
                  out: File::NULL)
      next "#{crate} does not build #{ENGINE_DESELECTED} — it reaches into the wasm engine"
    end

    engine_reached_violation(crate)
  end
end

# Which engine crates +crate+'s engine-free graph still reaches, as a
# violation string, or +nil+ when it reaches none. Runs inside the crate's
# workspace directory.
def engine_reached_violation(crate)
  tree = `cargo tree -p #{crate} #{ENGINE_DESELECTED} -e normal 2>/dev/null`
  pulled = tree.lines.drop(1).filter_map { |line| line[/[a-z0-9-]+(?= v[0-9])/] }
  found = pulled.uniq & ENGINE_CRATES
  return if found.empty?

  "#{crate}'s engine-free build still reaches an engine: #{found.join(", ")}"
end

namespace :gate do
  namespace :engine do
    desc "Check the Rust host SDK builds with no wasm engine in its dependency graph."
    task :optional do
      violations = ENGINE_ABSENT_TIERS.filter_map { |crate, dir| engine_absent_violation(crate, dir) }
      puts KobakoReport.gate(name: "gate:engine:optional",
                             ok_summary: "#{ENGINE_ABSENT_TIERS.size} tier reaches no wasm " \
                                         "engine with the engine deselected",
                             violations: violations, noun: "violation")
    end
  end
end
