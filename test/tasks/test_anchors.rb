# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/anchors"

# Unit coverage for the append-only anchor invariant (N-8): the checker
# that keeps +B-xx+ / +E-xx+ / +RX-xx+ allocations unique, contiguous, and
# resolvable once the +docs/behavior/+ split scatters definitions across
# files. Fixtures are hand-built doc fragments so each test states only the
# rule it is about — heading-defined anchors (+B+ / +RX+), table-defined
# anchors (+E+), tombstone prose, and reference tokens.
class KobakoAnchorsTest < Minitest::Test
  Anchors = KobakoAnchors

  def test_b_anchors_are_defined_by_their_markdown_heading
    text = "## B-01 — Construct\n\n## B-02 — Invoke\n"

    assert_equal [1, 2], Anchors.definitions(text, "B")
  end

  def test_e_anchors_are_defined_by_their_table_row_not_prose_references
    text = <<~MD
      | E-04 | guest raises | B-02 |
      | E-05 | compile fails | B-02 |

      The dispatch reuses E-04 when the entrypoint raises (E-11).
    MD

    assert_equal [4, 5], Anchors.definitions(text, "E"),
                 "a table row defines an E anchor; an inline (E-04) reference does not"
  end

  def test_tombstone_prose_marks_a_number_as_retired
    text = "E-14 is a retired anchor — permanently reserved and never reassigned (N-8)."

    assert_equal [14], Anchors.tombstones(text, "E")
  end

  def test_references_extract_anchor_tokens_including_range_endpoints
    text = "See B-07..B-12 and E-19; RX-03 and JS-08 cover it. Not an anchor: rev-2026."

    refs = Anchors.references(text)

    assert_includes refs, ["B", 7]
    assert_includes refs, ["B", 12]
    assert_includes refs, ["E", 19]
    assert_includes refs, ["RX", 3]
    assert_includes refs, ["JS", 8]
    refute_includes refs, ["E", 2026], "a hyphenated number inside a word is not an anchor reference"
  end

  def test_clean_corpus_reports_no_violations
    violations = Anchors.audit(
      def_sources: { "B" => { "lifecycle.md" => "## B-01 — x\n## B-02 — y\n" } },
      ref_sources: { "lifecycle.md" => "B-01 leads to B-02" },
      ceilings: { "B" => 2 }
    )

    assert_empty violations
  end

  def test_an_anchor_defined_in_two_files_is_a_duplicate_violation
    two_files = { "lifecycle.md" => "## B-01 — x\n", "dispatch.md" => "## B-01 — again\n" }

    violations = Anchors.audit(def_sources: { "B" => two_files }, ref_sources: {}, ceilings: { "B" => 1 })

    assert(violations.any? { |v| v.include?("B-01") && v.downcase.include?("duplicate") },
           "the same anchor in two files must be flagged so the split cannot re-allocate an ID")
  end

  def test_a_sequence_gap_without_a_tombstone_is_a_violation
    violations = Anchors.audit(
      def_sources: { "E" => { "errors.md" => "| E-01 | x |\n| E-03 | z |\n" } },
      ref_sources: {},
      ceilings: { "E" => 3 }
    )

    assert(violations.any? { |v| v.include?("E-02") },
           "a missing number with no retired tombstone breaks the contiguous sequence")
  end

  def test_a_sequence_gap_backed_by_a_tombstone_is_allowed
    text = "| E-01 | x |\n| E-03 | z |\nE-02 is a retired anchor — reserved (N-8).\n"

    violations = Anchors.audit(
      def_sources: { "E" => { "errors.md" => text } },
      ref_sources: { "errors.md" => "E-02 was removed" },
      ceilings: { "E" => 3 }
    )

    assert_empty violations,
                 "a tombstoned number is a legal hole and resolves references to it"
  end

  def test_a_ceiling_that_disagrees_with_the_highest_definition_is_a_violation
    violations = Anchors.audit(
      def_sources: { "B" => { "lifecycle.md" => "## B-01 — x\n## B-02 — y\n" } },
      ref_sources: {},
      ceilings: { "B" => 5 }
    )

    assert(violations.any? { |v| v.downcase.include?("ceiling") },
           "SPEC's stated ceiling must match the highest anchor actually defined")
  end

  def test_a_reference_to_an_undefined_anchor_is_dangling
    violations = Anchors.audit(
      def_sources: { "B" => { "lifecycle.md" => "## B-01 — x\n" } },
      ref_sources: { "readme.md" => "see B-99 for details" },
      ceilings: { "B" => 1 }
    )

    assert(violations.any? { |v| v.include?("B-99") },
           "a citation that resolves to no definition signals a typo or a stale link")
  end

  def test_ceilings_are_parsed_from_spec_refinement_prose
    text = "The current ceiling is B-50 / E-48; subsequent anchors take the next integer."

    assert_equal({ "B" => 50, "E" => 48 }, Anchors.parse_ceilings(text))
  end

  def test_rx_ceiling_is_derived_from_its_own_definitions
    text = "### RX-01 — a\n### RX-02 — b\n### RX-04 — d\nRX-03 is a retired anchor (N-8).\n"

    violations = Anchors.audit(
      def_sources: { "RX" => { "regexp.md" => text } },
      ref_sources: {},
      ceilings: {}
    )

    assert_empty violations,
                 "RX has no SPEC ceiling — its top is the highest RX defined, gaps need tombstones"
  end
end
