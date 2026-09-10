# frozen_string_literal: true

# Stage gate for the append-only anchor invariant (N-8). +rake gate:anchors+
# checks that every +F-xx+ / +J-xx+ / +N-x+ anchor +SPEC.md+ defines is
# defined once, runs contiguous (holes only where a retired tombstone
# declares one), and that every reference resolves. Behaviors are sumi
# scenarios, which +sumi verify+ holds to their own ids. Part of the release
# gate (+rake default+); the checker's unit coverage rides the test suite
# (+test/tasks/test_anchors.rb+).

require_relative "../support/anchors"
require_relative "../support/report"

# The tooling suites (+test/tasks/+, +test/bench/+) are excluded: their
# anchor-shaped tokens are hand-built fixtures, not references. +docs/spec/+
# is sumi's corpus, whose ids are its own to check.
ANCHOR_ROOT = File.expand_path("../..", __dir__)
ANCHOR_REF_GLOBS = FileList[
  "SPEC.md", "README.md", "docs/**/*.md", "test/**/*.rb", "benchmark/**/*.md"
].exclude(%r{/(target|vendor|tmp)/}, %r{\Atest/(tasks|bench)/}, %r{\Adocs/spec/})

namespace :gate do
  desc "Check F-/J-/N- anchors are unique, contiguous, and resolvable (N-8)."
  task :anchors do
    spec = KobakoAnchors.read_sources(FileList["SPEC.md"], ANCHOR_ROOT)
    violations = KobakoAnchors.audit(
      def_sources: { "F" => spec, "J" => spec, "N" => spec },
      ref_sources: KobakoAnchors.read_sources(ANCHOR_REF_GLOBS, ANCHOR_ROOT)
    )

    puts KobakoReport.gate(name: "gate:anchors",
                           ok_summary: "F/J/N unique, contiguous, and resolvable",
                           violations: violations.sort, noun: "violation")
  end
end
