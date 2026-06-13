# frozen_string_literal: true

# Stage gate for the append-only anchor invariant (N-8). +rake anchors+
# checks that every +B-xx+ / +E-xx+ / +RX-xx+ across the behavior spec is
# defined once, runs contiguous to the ceiling +SPEC.md+ states (holes only
# where a retired tombstone declares one), and that every reference resolves
# — the guard that keeps the +docs/behavior/+ split from re-allocating an
# ID. Part of the release gate (+rake default+); +rake anchors:test+ runs
# the checker's own unit coverage.

require_relative "support/anchors"

# Anchor definitions live in the behavior spec (+B+ / +E+) and the regexp
# spec (+RX+). Anchors are cited only where traceability belongs — the spec
# corpus and the tests that verify each behavior; implementation comments
# state intent rather than anchors, so the source trees are not scanned.
ANCHOR_ROOT = File.expand_path("..", __dir__)
ANCHOR_DEF_BEHAVIOR = FileList["docs/behavior/*.md"]
ANCHOR_DEF_REGEXP = FileList["docs/regexp.md"]
ANCHOR_REF_GLOBS = FileList[
  "SPEC.md", "README.md", "docs/**/*.md", "test/**/*.rb", "benchmark/**/*.md"
].exclude(%r{/(target|vendor|tmp)/})

namespace :anchors do
  desc "Run the anchor checker's unit coverage."
  task :test do
    sh "bundle exec ruby tasks/support/anchors_test.rb"
  end
end

desc "Check B-/E-/RX- anchors are unique, contiguous, and resolvable (N-8)."
task :anchors do
  behavior = KobakoAnchors.read_sources(ANCHOR_DEF_BEHAVIOR, ANCHOR_ROOT)
  violations = KobakoAnchors.audit(
    def_sources: {
      "B" => behavior, "E" => behavior,
      "RX" => KobakoAnchors.read_sources(ANCHOR_DEF_REGEXP, ANCHOR_ROOT)
    },
    ref_sources: KobakoAnchors.read_sources(ANCHOR_REF_GLOBS, ANCHOR_ROOT),
    ceilings: KobakoAnchors.parse_ceilings(File.read("SPEC.md"))
  )

  if violations.empty?
    puts "anchors: OK — B/E/RX unique, contiguous, and resolvable"
  else
    violations.sort.each { |v| warn "  #{v}" }
    abort "anchors: #{violations.size} violation(s)"
  end
end
