# frozen_string_literal: true

# Wire-symmetric peer gate (docs/wire-contract.md § Wire-Symmetric
# Peers): the payload type names of +lib/+ and +crates/kobako-codec+ must
# match one another, with a one-sided name carried by the Accepted
# asymmetries ledger. Names are the whole reading — the bytes those types
# encode are already held to each other by the round-trip fuzz, and a type
# only one peer grows is the shape no generated case reaches.
#
# Only the payload layer has two type-owning implementations; the core
# envelope has a single one in +crates/kobako-transport+, pinned by golden
# vectors derived from docs/wire/envelope.md. The guest's mruby-value walk
# is no third peer for this gate to read: it names no payload type of its
# own, consuming +kobako-codec+'s, so what it can get wrong is a value's
# fidelity rather than a type's shape — held instead by the guest
# round-trip fuzz (+test/fuzz/test_guest_value_fuzz.rb+). The comparator's
# unit coverage rides the test suite (+test/tasks/test_wire_symmetry.rb+).

require_relative "../support/anchors"
require_relative "../support/wire_symmetry"
require_relative "../support/report"

WIRE_SYMMETRY_ROOT = File.expand_path("../..", __dir__)
WIRE_SYMMETRY_DOC = "docs/wire-contract.md"
# Every inventory scans its whole tier — façade file plus the recursive
# subtree — so a codec-bearing type that moves within the tier cannot
# vanish from the gate, even when both peers move together. The patterns
# are globs rather than named files so a tier one side has emptied reports
# as a one-sided inventory instead of aborting the gate.
WIRE_RUBY_TRANSPORT = FileList["lib/kobako/transport*.rb", "lib/kobako/transport/**/*.rb",
                               "lib/kobako/payload*.rb", "lib/kobako/payload/**/*.rb"]
WIRE_RUST_TRANSPORT = FileList["crates/kobako-codec/src/**/transport*.rs",
                               "crates/kobako-codec/src/**/transport/**/*.rs",
                               "crates/kobako-codec/src/**/payload*.rs",
                               "crates/kobako-codec/src/**/payload/**/*.rs"]

# Both sides' inventories, keyed for +KobakoWireSymmetry.violations+.
def wire_symmetry_inventories
  {
    ruby_types: KobakoWireSymmetry.ruby_types(KobakoAnchors.read_sources(WIRE_RUBY_TRANSPORT, WIRE_SYMMETRY_ROOT)),
    rust_types: KobakoWireSymmetry.rust_types(KobakoAnchors.read_sources(WIRE_RUST_TRANSPORT, WIRE_SYMMETRY_ROOT))
  }
end

namespace :gate do
  namespace :wire do
    desc "Check lib/ and kobako-codec payload inventories match (docs/wire-contract.md § Wire-Symmetric Peers)."
    task :symmetry do
      accepted = KobakoWireSymmetry.accepted_asymmetries(File.read(WIRE_SYMMETRY_DOC))
      abort "gate:wire:symmetry: #{WIRE_SYMMETRY_DOC} has no 'Accepted asymmetries' block" unless accepted

      inventories = wire_symmetry_inventories
      violations = KobakoWireSymmetry.violations(**inventories, accepted: accepted)
      ok_summary = "#{inventories[:ruby_types].size} payload type(s) on both sides " \
                   "(#{inventories[:ruby_types].join(", ")}), #{accepted.size} accepted asymmetries"
      puts KobakoReport.gate(name: "gate:wire:symmetry", ok_summary: ok_summary,
                             violations: violations, noun: "divergence")
    end
  end
end
