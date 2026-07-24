# frozen_string_literal: true

# +rake gate:gvl:isolation+ — the structural guard behind GVL release (B-64).
# The wasmtime driver the GVL-released span calls must declare no +magnus+, so
# a released span cannot reach a Ruby VALUE by construction; adding the
# dependency is the only way to break the invariant, and this gate rejects it.
# Reader unit coverage rides test/tasks/test_gvl_isolation.rb.

require_relative "../support/gvl_isolation"
require_relative "../support/report"

namespace :gate do
  namespace :gvl do
    desc "Check the wasmtime driver (kobako-wasmtime) declares no magnus (GVL-release safety)."
    task :isolation do
      manifest = "crates/kobako-wasmtime/Cargo.toml"
      mentions = KobakoGvlIsolation.magnus_mentions(File.read(manifest))

      puts KobakoReport.gate(name: "gate:gvl:isolation",
                             ok_summary: "kobako-wasmtime links no magnus (GVL-released span stays VALUE-free)",
                             violations: mentions.map { |line| "#{manifest}: #{line}" },
                             noun: "magnus mention")
    end
  end
end
