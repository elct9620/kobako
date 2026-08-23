# frozen_string_literal: true

# Glossary consistency gate: the vocabulary holds together on its own terms.
# Reader unit coverage rides test/tasks/test_spec_glossary.rb.
#
# sumi reads the same file and answers the questions that need the corpus —
# rendering the page, and reporting each line a rejected name is used on. What
# is left here is what can be settled from the declaration alone, and neither
# of those two mistakes is one sumi would report: it would faithfully scan for
# a word that is itself one of our terms, and flag every legitimate use of it.
#
# Whether a word the corpus uses is a concept missing from the glossary is not
# asked by either: `Fault` belongs and `Symbol` does not, and no lexical rule
# separates them. That gap is filled by reading, not by blocking a release.

require_relative "../support/spec/glossary"
require_relative "../support/report"

namespace :gate do
  namespace :spec do
    desc "Check each vocabulary declares a concept once and rejects no name of its own."
    task :glossary do
      vocabularies = KobakoSpec::Glossary.load
      violations = KobakoSpec::Glossary.violations(vocabularies)

      puts KobakoReport.gate(name: "gate:spec:glossary",
                             ok_summary: "#{KobakoSpec::Glossary.terms(vocabularies).size} concept(s), " \
                                         "#{KobakoSpec::Glossary.rejections(vocabularies).size} rejected name(s) " \
                                         "across #{vocabularies.size} vocabular#{vocabularies.size == 1 ? "y" : "ies"}",
                             violations: violations, noun: "inconsistency")
    end
  end
end
