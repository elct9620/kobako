# frozen_string_literal: true

# Citation-profile instrument for the behavior anchors
# (docs/anchor-coverage.md): prints the thin and most-cited ends of the
# per-anchor profile, and fails when a zero-cited anchor lacks its
# Pending entry or a Pending entry has gone stale. Definition sources
# are the +ANCHOR_DEF_*+ corpus +rake anchors+ audits; +rake
# anchors:coverage:test+ runs the reader's own unit coverage.

require_relative "support/anchor_coverage"

ANCHOR_COVERAGE_DOC = "docs/anchor-coverage.md"
ANCHOR_COVERAGE_TESTS = FileList["test/**/*.rb"]

namespace :anchors do
  namespace :coverage do
    desc "Run the coverage reader's unit coverage."
    task :test do
      sh "bundle exec ruby tasks/support/anchor_coverage_test.rb"
    end
  end

  desc "Report the per-anchor citation profile and check the Pending ledger (docs/anchor-coverage.md)."
  task :coverage do
    behavior = KobakoAnchors.read_sources(ANCHOR_DEF_BEHAVIOR, ANCHOR_ROOT)
    profile = KobakoAnchorCoverage.profile(
      def_sources: {
        "B" => behavior, "E" => behavior,
        "RX" => KobakoAnchors.read_sources(ANCHOR_DEF_REGEXP, ANCHOR_ROOT),
        "JS" => KobakoAnchors.read_sources(ANCHOR_DEF_JSON, ANCHOR_ROOT)
      },
      test_sources: KobakoAnchors.read_sources(ANCHOR_COVERAGE_TESTS, ANCHOR_ROOT)
    )
    pending = KobakoAnchorCoverage.pending_anchors(File.read(ANCHOR_COVERAGE_DOC))
    abort "anchors:coverage: #{ANCHOR_COVERAGE_DOC} has no 'Pending anchors' block" unless pending

    puts "thin (at most one citing file):"
    KobakoAnchorCoverage.thin(profile).each do |anchor, files|
      detail = files.first || (pending.include?(anchor) ? "pending" : "UNCITED")
      puts format("  %<anchor>-6s %<detail>s", anchor: anchor, detail: detail)
    end
    puts "most cited:"
    KobakoAnchorCoverage.top(profile).each do |anchor, files|
      puts format("  %<anchor>-6s %<count>d files", anchor: anchor, count: files.size)
    end

    violations = KobakoAnchorCoverage.violations(profile, pending)
    if violations.empty?
      puts "anchors:coverage: OK — #{profile.size} anchors, #{pending.size} pending"
    else
      violations.each { |violation| warn "  anchors:coverage: #{violation}" }
      abort "anchors:coverage: #{violations.size} problem(s)"
    end
  end
end
