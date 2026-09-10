# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/anchors"

# Unit coverage for the append-only anchor invariant (N-8): the checker
# that keeps SPEC.md's own +F-xx+ / +J-xx+ / +N-x+ anchors unique,
# contiguous, and resolvable. Fixtures are hand-built doc fragments so each
# test states only the rule it is about — table-defined anchors (+F+ / +N+),
# heading-defined anchors (+J+), tombstone prose, and reference tokens.
class KobakoAnchorsTest < Minitest::Test
  Anchors = KobakoAnchors

  def test_f_and_n_anchors_are_defined_by_their_table_row_not_prose_references
    text = <<~MD
      | F-01 | Sandbox instantiation | Host Gem |
      | N-1 | Role names are PascalCase | All |

      Instantiation (F-01) follows N-1.
    MD

    assert_equal [1], Anchors.definitions(text, "F"),
                 "a table row defines an F anchor; an inline (F-01) reference does not"
    assert_equal [1], Anchors.definitions(text, "N"),
                 "a table row defines an N anchor; an inline N-1 reference does not"
  end

  def test_j_anchors_are_defined_by_their_heading
    assert_equal [1], Anchors.definitions("#### J-01 — LLM agent author runs code\n", "J"),
                 "a J heading through definitions must define J-01"
  end

  def test_tombstone_prose_marks_a_number_as_retired
    text = "F-03 is a retired feature anchor — permanently reserved and never reassigned (N-8)."

    assert_equal [3], Anchors.tombstones(text, "F"),
                 "retirement prose through tombstones must mark its number as a legal hole"
  end

  def test_references_extract_anchor_tokens_including_range_endpoints
    text = "F-10..F-12 cover it (J-06, N-8). Not one: rev-2026."

    assert_equal [["F", 10], ["F", 12], ["J", 6], ["N", 8]], Anchors.references(text),
                 "every family token must extract; a hyphenated number inside a word is not an anchor"
  end

  # Sumi scenarios number in three digits and some share a series letter
  # with these families, so a scenario id read as a citation would invent a
  # dangling anchor.
  def test_references_skip_three_digit_scenario_ids
    assert_empty Anchors.references("see [`J-010`](journeys.md) and # @behavior J-001 N-100"),
                 "a three-digit scenario id through references must cite no anchor"
  end

  def test_clean_corpus_reports_no_violations
    violations = audit("J", { "SPEC.md" => "#### J-01 — x\n#### J-02 — y\n" },
                       refs: { "README.md" => "J-01 leads to J-02" })

    assert_empty violations,
                 "a contiguous, resolvable corpus through audit must report no violations"
  end

  def test_an_anchor_defined_in_two_files_is_a_duplicate_violation
    two_files = { "SPEC.md" => "| F-01 | x |\n", "docs/other.md" => "| F-01 | again |\n" }

    violations = audit("F", two_files)

    assert(violations.any? { |v| v.include?("F-01") && v.downcase.include?("duplicate") },
           "the same anchor in two files must be flagged so a number is never allocated twice")
  end

  def test_a_sequence_gap_without_a_tombstone_is_a_violation
    violations = audit("F", { "SPEC.md" => "| F-01 | x |\n| F-03 | z |\n" })

    assert(violations.any? { |v| v.include?("F-02") },
           "a missing number with no retired tombstone breaks the contiguous sequence")
  end

  def test_a_sequence_gap_backed_by_a_tombstone_is_allowed
    text = "| F-01 | x |\n| F-03 | z |\nF-02 is a retired anchor — reserved (N-8).\n"

    violations = audit("F", { "SPEC.md" => text }, refs: { "README.md" => "F-02 was removed" })

    assert_empty violations,
                 "a tombstoned number is a legal hole and resolves references to it"
  end

  def test_a_reference_to_an_undefined_anchor_is_dangling
    violations = audit("F", { "SPEC.md" => "| F-01 | x |\n" },
                       refs: { "README.md" => "see F-99 for details" })

    assert(violations.any? { |v| v.include?("F-99") },
           "a citation that resolves to no definition signals a typo or a stale link")
  end

  private

  # Every fixture corpus here exercises a single family, so the audit
  # call unwraps to one prefix.
  def audit(prefix, files, refs: {})
    Anchors.audit(def_sources: { prefix => files }, ref_sources: refs)
  end
end
