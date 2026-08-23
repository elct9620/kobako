# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/spec/glossary"

# Unit coverage for the glossary reader. Both checks exist because of mistakes
# a reviewer already made, and neither is one sumi would report — it would
# faithfully scan for a word that is itself one of our terms, flagging every
# legitimate use of it. The per-vocabulary scoping is what lets a subdomain
# give a concept its own definition without either check firing.
class KobakoSpecGlossaryTest < Minitest::Test
  Glossary = KobakoSpec::Glossary

  def vocabulary(*terms, name: nil)
    declared = { "include" => ["SPEC.md"], "terms" => terms }
    name ? declared.merge("name" => name) : declared
  end

  def term(name, definition: "A concept.", rejects: nil)
    declared = { "term" => name, "definition" => definition }
    rejects ? declared.merge("not" => [{ "term" => rejects, "reason" => "Already covered." }]) : declared
  end

  def test_a_term_declared_twice_in_one_vocabulary_is_reported
    vocabularies = [vocabulary(term("Service"), term("Service", definition: "Something else."))]

    assert_equal ["Global: Service declared 2 times"], Glossary.violations(vocabularies),
                 "the same term declared under two definitions through .violations must be reported, since a " \
                 "name answering to two concepts is what the one-concept-one-name rule forbids"
  end

  def test_rejecting_a_word_that_is_itself_a_term_is_reported
    vocabularies = [vocabulary(term("Execution", rejects: "Invocation"), term("Invocation"))]

    assert_equal ["Global: Execution rejects \"Invocation\", which is a declared term"],
                 Glossary.violations(vocabularies),
                 "a rejection naming another declared term through .violations must be reported, because both " \
                 "words are ours and a scan for one would flag every legitimate use of the other"
  end

  def test_a_subdomain_redefining_a_global_term_is_not_a_duplicate
    vocabularies = [vocabulary(term("Codec")), vocabulary(term("Codec", definition: "Something else."), name: "Guest")]

    assert_empty Glossary.violations(vocabularies),
                 "one concept declared once in Global and again in a subdomain through .violations must be " \
                 "reported as nothing, since telling those two apart is the whole reason a subdomain exists"
  end

  def test_a_subdomain_names_itself_in_what_it_reports
    vocabularies = [vocabulary(term("Codec"), term("Codec", definition: "Something else."), name: "Guest")]

    assert_equal ["Guest: Codec declared 2 times"], Glossary.violations(vocabularies),
                 "an inconsistency inside a named vocabulary through .violations must carry that name, so a " \
                 "reader knows which of several vocabularies to open"
  end
end
