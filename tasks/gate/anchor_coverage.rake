# frozen_string_literal: true

# Citation-profile instrument for the behavior anchors
# (docs/anchor-coverage.md): prints the thin and most-cited ends of the
# per-anchor profile, and fails when a zero-cited anchor lacks its
# Pending entry or a Pending entry has gone stale. Definition sources
# are the +ANCHOR_DEF_*+ corpus +rake anchors+ audits; the reader's unit
# coverage rides the test suite (+test/tasks/test_anchor_coverage.rb+).

require_relative "support/anchor_coverage"
require_relative "support/report"

ANCHOR_COVERAGE_DOC = "docs/anchor-coverage.md"
# The tooling suites are excluded as citation sources: their
# anchor-shaped tokens are hand-built fixtures, not witnesses.
ANCHOR_COVERAGE_TESTS = FileList["test/**/*.rb"].exclude(%r{\Atest/(tasks|bench)/})

# The full citation profile over the same definition corpus +rake
# anchors+ audits (+anchor_behavior_def_sources+; the SPEC-local
# F/J/N families stay out — their witnesses are not test citations).
def anchor_coverage_profile
  KobakoAnchorCoverage.profile(
    def_sources: anchor_behavior_def_sources,
    test_sources: KobakoAnchors.read_sources(ANCHOR_COVERAGE_TESTS, ANCHOR_ROOT)
  )
end

namespace :anchors do
  desc "Report the per-anchor citation profile and check the Pending ledger (docs/anchor-coverage.md)."
  task :coverage do
    doc = File.read(ANCHOR_COVERAGE_DOC)
    profile = anchor_coverage_profile
    pending = KobakoAnchorCoverage.pending_anchors(doc)
    abort "anchors:coverage: #{ANCHOR_COVERAGE_DOC} has no 'Pending anchors' block" unless pending

    e2e_witnessed = KobakoAnchorCoverage.e2e_witnessed_anchors(doc)
    abort "anchors:coverage: #{ANCHOR_COVERAGE_DOC} has no 'E2E-witnessed anchors' block" unless e2e_witnessed

    puts KobakoAnchorCoverage.report_lines(profile, pending)
    violations = KobakoAnchorCoverage.violations(profile, pending, e2e_witnessed)
    puts KobakoReport.gate(name: "anchors:coverage",
                           ok_summary: "#{profile.size} anchors, #{pending.size} pending",
                           violations: violations)
  end
end
