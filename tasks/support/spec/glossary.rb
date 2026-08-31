# frozen_string_literal: true

module KobakoSpec
  # Reader behind +rake gate:spec:glossary+: the structural checks sumi does
  # not make about the vocabulary it reads.
  #
  # sumi owns what it can answer from the corpus — reporting each line where a
  # rejected name is used. What it never asks is whether the declaration makes
  # sense on its own terms, and the two ways it can fail both flag legitimate
  # prose rather than the drift a rejection exists to catch: a name answering
  # to two concepts, which sumi silently resolves to the later one, and a
  # rejection naming a word that is itself one of our terms, which sumi
  # faithfully reports at every legitimate use of it.
  #
  # Both are asked per vocabulary rather than across the file, because one
  # concept legitimately carries different definitions in different subdomains
  # — that separation is the reason a subdomain exists.
  module Glossary
    # Where the vocabularies are declared. sumi reads this same file.
    SOURCE = "docs/spec/glossary.md"

    module_function

    # The declared vocabularies, in file order — the widest first, then each
    # subdomain, which is the order a later term overrides an earlier one in.
    def load
      parse(File.read(SOURCE))
    end

    # The vocabularies +text+ declares, each opened by a +##+ heading.
    def parse(text)
      text.split(/^##(?!#)[ \t]*/).drop(1).map { |section| parse_vocabulary(section) }
    end

    # One +##+ section as a vocabulary — its heading names it, its +###+
    # headings are its terms.
    def parse_vocabulary(section)
      name, body = section.split("\n", 2)
      { "name" => name.strip, "terms" => parse_terms(body.to_s) }
    end
    private_class_method :parse_vocabulary

    # The terms a vocabulary's body declares. +Includes+ is the one +###+
    # heading that is reserved rather than a term.
    def parse_terms(body)
      body.split(/^###(?!#)[ \t]*/).drop(1).filter_map do |block|
        heading, rest = block.split("\n", 2)
        next if heading.strip == "Includes"

        { "term" => heading.strip, "not" => parse_rejections(rest.to_s) }
      end
    end
    private_class_method :parse_terms

    # A term's rejected names, written +- `word` — reason+ under its
    # +#### Rejected+ heading.
    def parse_rejections(block)
      _, rejected = block.split(/^####[ \t]+Rejected[ \t]*$/, 2)
      rejected.to_s.scan(/^-[ \t]+`([^`]+)`[ \t]+—[ \t]+(.+)$/)
              .map { |word, reason| { "term" => word, "reason" => reason.strip } }
    end
    private_class_method :parse_rejections

    # Every way +vocabularies+ can be wrong on its own terms, as reader-facing
    # lines.
    def violations(vocabularies)
      vocabularies.flat_map { |vocabulary| duplicates(vocabulary) + self_rejections(vocabulary) }
    end

    # +[term, word, reason]+ for each rejected name, in declaration order.
    def rejections(vocabularies)
      vocabularies.flat_map { |vocabulary| rejected_in(vocabulary) }
    end

    # Every declared term, in declaration order.
    def terms(vocabularies)
      vocabularies.flat_map { |vocabulary| vocabulary.fetch("terms").map { |declared| declared["term"] } }
    end

    # +[term, word, reason]+ for one vocabulary's rejected names.
    def rejected_in(vocabulary)
      vocabulary.fetch("terms").flat_map do |declared|
        (declared["not"] || []).map { |rejection| [declared["term"], rejection["term"], rejection["reason"]] }
      end
    end
    private_class_method :rejected_in

    # How a vocabulary names itself in a finding. Global carries no name, so
    # the one it is given here is the one the reader already calls it.
    def label(vocabulary)
      vocabulary["name"] || "Global"
    end
    private_class_method :label

    # Terms declared more than once within one vocabulary: a name answering to
    # two concepts is what the one-concept-one-name rule forbids.
    def duplicates(vocabulary)
      vocabulary.fetch("terms")
                .map { |declared| declared["term"] }
                .tally.select { |_, count| count > 1 }
                .map { |term, count| "#{label(vocabulary)}: #{term} declared #{count} times" }
    end
    private_class_method :duplicates

    # Rejected names that are themselves declared in the same vocabulary. Both
    # words being ours means the reader tells them apart from the definitions;
    # scanning for one would flag every legitimate use of the other.
    def self_rejections(vocabulary)
      declared = vocabulary.fetch("terms").map { |term| term["term"] }
      rejected_in(vocabulary).filter_map do |term, word, _|
        "#{label(vocabulary)}: #{term} rejects #{word.inspect}, which is a declared term" if declared.include?(word)
      end
    end
    private_class_method :self_rejections
  end
end
