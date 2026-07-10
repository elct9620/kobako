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
# (+RX+), the JSON spec (+JS+), and SPEC.md itself for the SPEC-local
# families (+F+ features, +J+ journeys, +N+ naming principles). Anchors are
# cited only where traceability
# belongs — the spec corpus and the tests that verify each behavior;
# implementation comments state intent rather than anchors, so the source
# trees are not scanned. The tooling suites (+test/tasks/+, +test/bench/+)
# are excluded: their anchor-shaped tokens are hand-built fixtures, not
# references.
ANCHOR_ROOT = File.expand_path("..", __dir__)
ANCHOR_DEF_BEHAVIOR = FileList["docs/behavior/*.md"]
ANCHOR_DEF_REGEXP = FileList["docs/regexp.md"]
ANCHOR_DEF_JSON = FileList["docs/json.md"]
ANCHOR_REF_GLOBS = FileList[
  "SPEC.md", "README.md", "docs/**/*.md", "test/**/*.rb", "benchmark/**/*.md"
].exclude(%r{/(target|vendor|tmp)/}, %r{\Atest/(tasks|bench)/})

# The behavior-family definition corpus (+B+ / +E+ share the behavior
# split; +RX+ / +JS+ are topic-doc-local) — one assembly audited by the
# anchors gate and profiled by anchors:coverage, so a new family is
# wired in one place.
def anchor_behavior_def_sources
  behavior = KobakoAnchors.read_sources(ANCHOR_DEF_BEHAVIOR, ANCHOR_ROOT)
  { "B" => behavior, "E" => behavior,
    "RX" => KobakoAnchors.read_sources(ANCHOR_DEF_REGEXP, ANCHOR_ROOT),
    "JS" => KobakoAnchors.read_sources(ANCHOR_DEF_JSON, ANCHOR_ROOT) }
end

desc "Check B-/E-/RX-/JS-/F-/J-/N- anchors are unique, contiguous, and resolvable (N-8)."
task :anchors do
  spec = KobakoAnchors.read_sources(FileList["SPEC.md"], ANCHOR_ROOT)
  ceilings = KobakoAnchors.parse_ceilings(File.read("SPEC.md"))
  violations = KobakoAnchors.ceiling_statement_violations(ceilings) + KobakoAnchors.audit(
    def_sources: anchor_behavior_def_sources.merge("F" => spec, "J" => spec, "N" => spec),
    ref_sources: KobakoAnchors.read_sources(ANCHOR_REF_GLOBS, ANCHOR_ROOT),
    ceilings: ceilings
  )

  if violations.empty?
    puts "anchors: OK — B/E/RX/JS/F/J/N unique, contiguous, and resolvable"
  else
    violations.sort.each { |v| warn "  #{v}" }
    abort "anchors: #{violations.size} violation(s)"
  end
end
