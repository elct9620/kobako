# frozen_string_literal: true

# Citation-profile instrument for the behavior anchors
# (docs/anchor-coverage.md): prints the thin and most-cited ends of the
# per-anchor profile, and fails when a zero-cited anchor lacks its
# Pending entry or a Pending entry has gone stale. Definition sources
# are the +ANCHOR_DEF_*+ corpus +rake anchors+ audits; the reader's unit
# coverage rides the test suite (+test/tasks/test_anchor_coverage.rb+).

require_relative "support/anchor_coverage"

ANCHOR_COVERAGE_DOC = "docs/anchor-coverage.md"
ANCHOR_COVERAGE_TESTS = FileList["test/**/*.rb"]

# The full citation profile over the same definition corpus +rake
# anchors+ audits.
def anchor_coverage_profile
  behavior = KobakoAnchors.read_sources(ANCHOR_DEF_BEHAVIOR, ANCHOR_ROOT)
  KobakoAnchorCoverage.profile(
    def_sources: {
      "B" => behavior, "E" => behavior,
      "RX" => KobakoAnchors.read_sources(ANCHOR_DEF_REGEXP, ANCHOR_ROOT),
      "JS" => KobakoAnchors.read_sources(ANCHOR_DEF_JSON, ANCHOR_ROOT)
    },
    test_sources: KobakoAnchors.read_sources(ANCHOR_COVERAGE_TESTS, ANCHOR_ROOT)
  )
end

namespace :anchors do
  desc "Report the per-anchor citation profile and check the Pending ledger (docs/anchor-coverage.md)."
  task :coverage do
    profile = anchor_coverage_profile
    pending = KobakoAnchorCoverage.pending_anchors(File.read(ANCHOR_COVERAGE_DOC))
    abort "anchors:coverage: #{ANCHOR_COVERAGE_DOC} has no 'Pending anchors' block" unless pending

    puts KobakoAnchorCoverage.report_lines(profile, pending)
    violations = KobakoAnchorCoverage.violations(profile, pending)
    if violations.empty?
      puts "anchors:coverage: OK — #{profile.size} anchors, #{pending.size} pending"
    else
      violations.each { |violation| warn "  anchors:coverage: #{violation}" }
      abort "anchors:coverage: #{violations.size} problem(s)"
    end
  end
end
