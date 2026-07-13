# frozen_string_literal: true

# Wire-symmetric peer gate (docs/wire-contract.md § Wire-Symmetric
# Peers): the transport envelope types and ext type codes of +lib/+ and
# +crates/kobako-codec+ must match name-for-name, with one-sided entries
# carried by the Accepted asymmetries ledger. The comparator's unit
# coverage rides the test suite (+test/tasks/test_wire_symmetry.rb+).

require_relative "../support/anchors"
require_relative "../support/wire_symmetry"
require_relative "../support/report"

WIRE_SYMMETRY_ROOT = File.expand_path("../..", __dir__)
WIRE_SYMMETRY_DOC = "docs/wire-contract.md"
# Every inventory scans its whole tier — façade file plus the recursive
# subtree — so an envelope or registration that moves within the tier
# cannot vanish from the gate, even when both peers move together.
WIRE_RUBY_TRANSPORT = FileList["lib/kobako/transport.rb", "lib/kobako/transport/**/*.rb"]
WIRE_RUST_TRANSPORT = FileList["crates/kobako-codec/src/transport.rs", "crates/kobako-codec/src/transport/**/*.rs"]
WIRE_RUBY_EXT = FileList["lib/kobako/codec.rb", "lib/kobako/codec/**/*.rb"]
WIRE_RUST_EXT = FileList["crates/kobako-codec/src/**/*.rs"]

# Both sides' inventories, keyed for +KobakoWireSymmetry.violations+.
def wire_symmetry_inventories
  {
    ruby_types: KobakoWireSymmetry.ruby_types(KobakoAnchors.read_sources(WIRE_RUBY_TRANSPORT, WIRE_SYMMETRY_ROOT)),
    rust_types: KobakoWireSymmetry.rust_types(KobakoAnchors.read_sources(WIRE_RUST_TRANSPORT, WIRE_SYMMETRY_ROOT)),
    ruby_ext: KobakoWireSymmetry.ruby_ext_codes(WIRE_RUBY_EXT.map { |path| File.read(path) }.join),
    rust_ext: KobakoWireSymmetry.rust_ext_codes(WIRE_RUST_EXT.map { |path| File.read(path) }.join)
  }
end

namespace :gate do
  namespace :wire do
    desc "Check lib/ and kobako-codec wire inventories match (docs/wire-contract.md § Wire-Symmetric Peers)."
    task :symmetry do
      accepted = KobakoWireSymmetry.accepted_asymmetries(File.read(WIRE_SYMMETRY_DOC))
      abort "gate:wire:symmetry: #{WIRE_SYMMETRY_DOC} has no 'Accepted asymmetries' block" unless accepted

      inventories = wire_symmetry_inventories
      violations = KobakoWireSymmetry.violations(**inventories, accepted: accepted)
      ok_summary = "#{inventories[:ruby_types].size} envelope types on both sides " \
                   "(#{inventories[:ruby_types].join(", ")}), #{accepted.size} accepted asymmetries"
      puts KobakoReport.gate(name: "gate:wire:symmetry", ok_summary: ok_summary,
                             violations: violations, noun: "divergence")
    end
  end
end
