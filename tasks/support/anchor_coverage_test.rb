# frozen_string_literal: true

require "minitest/autorun"

require_relative "anchor_coverage"

# Unit coverage for the citation-profile reader (docs/anchor-coverage.md):
# the profile counts distinct citing files rather than mentions, the
# Pending list parses only from its fenced block, and both gate rules
# fire — a zero-cited anchor without a Pending entry, and a Pending
# entry a test now cites.
class KobakoAnchorCoverageTest < Minitest::Test
  Coverage = KobakoAnchorCoverage

  DEFS = {
    "B" => { "docs/behavior/x.md" => "## B-01 — One\n\n## B-02 — Two\n" },
    "E" => { "docs/behavior/e.md" => "| E-01 | boom | B-01 |\n" }
  }.freeze

  def test_profile_counts_distinct_citing_files_not_mentions
    tests = { "test/a_test.rb" => "B-01 B-01 B-01", "test/b_test.rb" => "covers B-01" }

    profile = Coverage.profile(def_sources: DEFS, test_sources: tests)

    assert_equal ["test/a_test.rb", "test/b_test.rb"], profile["B-01"],
                 "three mentions in one file must count as one citing file"
  end

  def test_profile_lists_every_defined_anchor_even_when_uncited
    profile = Coverage.profile(def_sources: DEFS, test_sources: {})

    assert_equal [], profile["E-01"],
                 "a defined anchor with no citing test must appear with an empty file list"
  end

  def test_pending_anchors_parse_only_the_fenced_block
    markdown = <<~MD
      ## Pending anchors

      Prose mentioning E-99 does not count:

      ```
      E-01
      ```
    MD

    assert_equal ["E-01"], Coverage.pending_anchors(markdown),
                 "only anchors inside the fenced block are Pending entries"
  end

  def test_pending_anchors_is_nil_without_a_pending_block
    assert_nil Coverage.pending_anchors("# No such section\n")
  end

  def test_zero_cited_anchor_without_pending_entry_is_a_violation
    profile = { "B-01" => ["test/a_test.rb"], "E-01" => [] }

    violations = Coverage.violations(profile, [])

    assert_equal ["E-01 has no citing test and no Pending anchors entry"], violations
  end

  def test_pending_anchor_with_a_citing_test_is_stale
    profile = { "B-01" => ["test/a_test.rb"] }

    violations = Coverage.violations(profile, ["B-01"])

    assert_equal ["B-01 is cited by a test — drop it from Pending anchors"], violations
  end

  def test_pending_entry_silences_the_zero_cited_rule
    profile = { "E-01" => [] }

    assert_empty Coverage.violations(profile, ["E-01"])
  end

  def test_pending_entry_for_an_undefined_anchor_is_not_reported_stale
    profile = { "B-01" => ["test/a_test.rb"] }

    assert_empty Coverage.violations(profile, ["E-99"]),
                 "an undefined Pending anchor belongs to rake anchors' dangling check, not the stale rule"
  end

  def test_thin_lists_anchors_with_at_most_one_citing_file_in_anchor_order
    profile = { "E-01" => [], "B-02" => ["test/a_test.rb"], "B-01" => %w[test/a_test.rb test/b_test.rb] }

    assert_equal [["B-02", ["test/a_test.rb"]], ["E-01", []]], Coverage.thin(profile)
  end

  def test_report_lines_mark_pending_and_uncited_thin_anchors
    profile = { "E-01" => [], "E-02" => [], "B-01" => ["test/a_test.rb"] }

    lines = Coverage.report_lines(profile, ["E-01"])

    assert_includes lines, "  E-01   pending"
    assert_includes lines, "  E-02   UNCITED"
    assert_includes lines, "  B-01   test/a_test.rb"
  end

  def test_top_lists_the_most_cited_anchors_first
    profile = { "B-01" => ["test/a_test.rb"], "B-02" => %w[test/a_test.rb test/b_test.rb] }

    assert_equal [["B-02", %w[test/a_test.rb test/b_test.rb]], ["B-01", ["test/a_test.rb"]]],
                 Coverage.top(profile, limit: 2)
  end
end
