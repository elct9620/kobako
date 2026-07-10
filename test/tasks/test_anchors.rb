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

    assert_equal [1, 2], Anchors.definitions(text, "B"),
                 "Markdown headings through definitions must yield each B number once"
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

  # SPEC-local families: +F+ / +N+ define by table row like +E+, +J+ by
  # heading like +B+ — a typo'd F-99 / J-99 / N-9 reference must resolve
  # against these definitions instead of passing silently.
  def test_spec_local_families_define_by_table_row_or_heading
    table = "| F-01 | Sandbox instantiation | Host Gem |\n| N-1 | Role names are PascalCase | All |\n"

    assert_equal [1], Anchors.definitions(table, "F"), "an F table row through definitions must define F-01"
    assert_equal [1], Anchors.definitions(table, "N"), "an N table row through definitions must define N-1"
    assert_equal [1], Anchors.definitions("#### J-01 — LLM agent author runs code\n", "J"),
                 "a J heading through definitions must define J-01"
  end

  def test_tombstone_prose_marks_a_number_as_retired
    text = "E-14 is a retired anchor — permanently reserved and never reassigned (N-8)."

    assert_equal [14], Anchors.tombstones(text, "E"),
                 "retirement prose through tombstones must mark its number as a legal hole"
  end

  def test_references_extract_anchor_tokens_including_range_endpoints
    text = "See B-07..B-12 and E-19; RX-03, JS-08, and F-10 cover it (J-06, N-8). Not one: rev-2026."

    expected = [["B", 7], ["B", 12], ["E", 19], ["RX", 3], ["JS", 8], ["F", 10], ["J", 6], ["N", 8]]

    assert_equal expected, Anchors.references(text),
                 "every family token must extract; a hyphenated number inside a word is not an anchor"
  end

  def test_clean_corpus_reports_no_violations
    violations = audit("B", { "lifecycle.md" => "## B-01 — x\n## B-02 — y\n" },
                       refs: { "lifecycle.md" => "B-01 leads to B-02" }, ceilings: { "B" => 2 })

    assert_empty violations,
                 "a contiguous, resolvable corpus through audit must report no violations"
  end

  def test_an_anchor_defined_in_two_files_is_a_duplicate_violation
    two_files = { "lifecycle.md" => "## B-01 — x\n", "dispatch.md" => "## B-01 — again\n" }

    violations = audit("B", two_files, ceilings: { "B" => 1 })

    assert(violations.any? { |v| v.include?("B-01") && v.downcase.include?("duplicate") },
           "the same anchor in two files must be flagged so the split cannot re-allocate an ID")
  end

  def test_a_sequence_gap_without_a_tombstone_is_a_violation
    violations = audit("E", { "errors.md" => "| E-01 | x |\n| E-03 | z |\n" }, ceilings: { "E" => 3 })

    assert(violations.any? { |v| v.include?("E-02") },
           "a missing number with no retired tombstone breaks the contiguous sequence")
  end

  def test_a_sequence_gap_backed_by_a_tombstone_is_allowed
    text = "| E-01 | x |\n| E-03 | z |\nE-02 is a retired anchor — reserved (N-8).\n"

    violations = audit("E", { "errors.md" => text },
                       refs: { "errors.md" => "E-02 was removed" }, ceilings: { "E" => 3 })

    assert_empty violations,
                 "a tombstoned number is a legal hole and resolves references to it"
  end

  def test_a_ceiling_that_disagrees_with_the_highest_definition_is_a_violation
    violations = audit("B", { "lifecycle.md" => "## B-01 — x\n## B-02 — y\n" }, ceilings: { "B" => 5 })

    assert(violations.any? { |v| v.downcase.include?("ceiling") },
           "SPEC's stated ceiling must match the highest anchor actually defined")
  end

  def test_a_reference_to_an_undefined_anchor_is_dangling
    violations = audit("B", { "lifecycle.md" => "## B-01 — x\n" },
                       refs: { "readme.md" => "see B-99 for details" }, ceilings: { "B" => 1 })

    assert(violations.any? { |v| v.include?("B-99") },
           "a citation that resolves to no definition signals a typo or a stale link")
  end

  def test_ceilings_are_parsed_from_spec_refinement_prose
    text = "The current ceiling is B-50 / E-48; subsequent anchors take the next integer."

    assert_equal({ "B" => 50, "E" => 48 }, Anchors.parse_ceilings(text),
                 "the SPEC refinement prose through parse_ceilings must yield the stated B and E tops")
  end

  # The gate's own liveness: a reworded ceiling statement must fail the
  # audit, not silently disable ceiling and sequence enforcement.
  def test_an_unparseable_ceiling_statement_is_a_violation
    assert_empty Anchors.ceiling_statement_violations({ "B" => 50, "E" => 48 }),
                 "a parsed ceiling set through ceiling_statement_violations must report nothing"
    refute_empty Anchors.ceiling_statement_violations({}),
                 "an empty ceiling parse must surface as a violation instead of disarming the gate"
  end

  def test_rx_ceiling_is_derived_from_its_own_definitions
    text = "### RX-01 — a\n### RX-02 — b\n### RX-04 — d\nRX-03 is a retired anchor (N-8).\n"

    violations = audit("RX", { "regexp.md" => text })

    assert_empty violations,
                 "RX has no SPEC ceiling — its top is the highest RX defined, gaps need tombstones"
  end

  private

  # Every fixture corpus here exercises a single family, so the audit
  # call unwraps to one prefix.
  def audit(prefix, files, refs: {}, ceilings: {})
    Anchors.audit(def_sources: { prefix => files }, ref_sources: refs, ceilings: ceilings)
  end
end
