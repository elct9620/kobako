# frozen_string_literal: true

# Stage gate for the append-only anchor invariant (N-8). +rake anchors+
# checks that every +B-xx+ / +E-xx+ / +RX-xx+ / +JS-xx+ across the spec is
# defined once, runs contiguous to the ceiling +SPEC.md+ states (holes only
# where a retired tombstone declares one), and that every reference resolves
# — the guard that keeps the +docs/behavior/+ split from re-allocating an
# ID. Part of the release gate (+rake default+); the checker's unit
# coverage rides the test suite (+test/tasks/test_anchors.rb+).

require_relative "support/anchors"

# Anchor definitions live in the behavior spec (+B+ / +E+), the regexp spec
# (+RX+), and the JSON spec (+JS+). Anchors are cited only where traceability
# belongs — the spec corpus and the tests that verify each behavior;
# implementation comments state intent rather than anchors, so the source
# trees are not scanned.
ANCHOR_ROOT = File.expand_path("..", __dir__)
ANCHOR_DEF_BEHAVIOR = FileList["docs/behavior/*.md"]
ANCHOR_DEF_REGEXP = FileList["docs/regexp.md"]
ANCHOR_DEF_JSON = FileList["docs/json.md"]
ANCHOR_REF_GLOBS = FileList[
  "SPEC.md", "README.md", "docs/**/*.md", "test/**/*.rb", "benchmark/**/*.md"
].exclude(%r{/(target|vendor|tmp)/})

desc "Check B-/E-/RX-/JS- anchors are unique, contiguous, and resolvable (N-8)."
task :anchors do
  behavior = KobakoAnchors.read_sources(ANCHOR_DEF_BEHAVIOR, ANCHOR_ROOT)
  violations = KobakoAnchors.audit(
    def_sources: {
      "B" => behavior, "E" => behavior,
      "RX" => KobakoAnchors.read_sources(ANCHOR_DEF_REGEXP, ANCHOR_ROOT),
      "JS" => KobakoAnchors.read_sources(ANCHOR_DEF_JSON, ANCHOR_ROOT)
    },
    ref_sources: KobakoAnchors.read_sources(ANCHOR_REF_GLOBS, ANCHOR_ROOT),
    ceilings: KobakoAnchors.parse_ceilings(File.read("SPEC.md"))
  )

  if violations.empty?
    puts "anchors: OK — B/E/RX/JS unique, contiguous, and resolvable"
  else
    violations.sort.each { |v| warn "  #{v}" }
    abort "anchors: #{violations.size} violation(s)"
  end
end
