# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/spec/glossary"

# Unit coverage for the glossary readers. Two of the three checks exist
# because of mistakes a reviewer already made: rejecting a word that is
# itself one of our terms (which would flag every legitimate use of it),
# and rejecting a word the corpus still uses in another sense (which the
# scan cannot tell from a misuse). The third keeps the page that declares a
# rejection from counting as a use of it.
class KobakoSpecGlossaryTest < Minitest::Test
  Glossary = KobakoSpec::Glossary

  CLEAN = [
    { "term" => "Service", "definition" => "A host object the guest reaches by name." },
    { "term" => "Fault", "definition" => "The reason a Call is refused." }
  ].freeze

  def entry(term, definition: "A concept.", rejects: nil)
    declared = { "term" => term, "definition" => definition }
    rejects ? declared.merge("not" => [{ "word" => rejects, "why" => "Already covered." }]) : declared
  end

  def test_a_term_declared_twice_is_reported
    entries = [entry("Service"), entry("Service", definition: "Something else.")]

    assert_equal ["Service: declared 2 times"], Glossary.violations(entries, {}),
                 "the same term declared under two definitions through .violations must be reported, since a " \
                 "name answering to two concepts is what the one-concept-one-name rule forbids"
  end

  def test_rejecting_a_word_that_is_itself_a_term_is_reported
    entries = [entry("Execution", rejects: "Invocation"), entry("Invocation")]

    assert_equal ["Execution: rejects \"Invocation\", which is a declared term"], Glossary.violations(entries, {}),
                 "a rejection naming another declared term through .violations must be reported, because both " \
                 "words are ours and a scan for one would flag every legitimate use of the other"
  end

  def test_rejecting_a_word_the_corpus_still_uses_is_reported
    entries = [entry("Service", rejects: "tool")]
    sources = { "SPEC.md" => "The build tool fetches release tarballs.\n" }

    assert_equal ["Service: rejects \"tool\", still used in SPEC.md"], Glossary.violations(entries, sources),
                 "a rejection whose word the corpus still uses through .violations must be reported, since the " \
                 "scan cannot separate that use from the misuse the rejection exists to catch"
  end

  def test_a_word_inside_a_longer_one_is_not_a_use_of_it
    entries = [entry("Service", rejects: "tool")]
    sources = { "SPEC.md" => "The toolchain is pinned, and tooling fetches it.\n" }

    assert_empty Glossary.violations(entries, sources),
                 "a rejected word appearing only inside longer words through .violations must be reported as " \
                 "nothing, so a rejection is not defeated by unrelated vocabulary that merely contains it"
  end

  def test_the_rendered_page_does_not_count_as_a_use_of_what_it_rejects
    entries = [entry("Service", rejects: "adapter")]
    sources = { Glossary::OUTPUT => "| Service | `adapter` | Already covered. |\n" }

    assert_empty Glossary.violations(entries, sources),
                 "a rejected word appearing in the rendered glossary through .violations must be reported as " \
                 "nothing, because the page that declares a rejection is stating it rather than using it"
  end

  def test_entries_render_in_the_order_they_are_declared
    rows = Glossary.render(CLEAN).lines.grep(/^\| \*\*/)

    assert_equal %w[Service Fault], rows.map { |row| row[/\*\*(.+?)\*\*/, 1] },
                 "entries through .render must appear in declaration order, so the page a generator rewrites " \
                 "is byte-stable against a data file nobody reordered"
  end

  def test_a_glossary_rejecting_nothing_renders_no_rejection_table
    refute_includes Glossary.render(CLEAN), "## Rejected names",
                    "a glossary with no rejections through .render must omit the rejection section, so an " \
                    "empty table never reads as a complete record of what has been ruled out"
  end

  def test_a_rejection_renders_with_the_reason_that_keeps_it_from_being_reproposed
    rendered = Glossary.render([entry("Service", rejects: "adapter")])

    assert_includes rendered, "| Service | `adapter` | Already covered. |",
                    "a rejection through .render must carry its reason, since a rejected-name list without one " \
                    "is re-argued the next time somebody proposes the word"
  end
end
