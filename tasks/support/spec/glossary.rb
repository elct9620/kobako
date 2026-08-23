# frozen_string_literal: true

require "json"

module KobakoSpec
  # Reader behind +rake gate:spec:glossary+: the structural checks sumi does
  # not make about the vocabulary it reads.
  #
  # sumi owns the two things it can do from the source alone — writing the
  # glossary page, and reporting each line where a rejected name is used. What
  # it never asks is whether the declaration makes sense on its own terms, and
  # the two ways it can fail both flag legitimate prose rather than the drift a
  # rejection exists to catch: a name answering to two concepts, and a
  # rejection naming a word that is itself one of our terms.
  #
  # Both are asked per vocabulary rather than across the file, because one
  # concept legitimately carries different definitions in different subdomains
  # — that separation is the reason a subdomain exists.
  module Glossary
    # Where the vocabularies are declared. sumi reads this same file.
    SOURCE = ".spec/glossary.json"

    module_function

    # The declared vocabularies, in file order — Global first, then each
    # subdomain, which is the order a later term overrides an earlier one in.
    def load
      JSON.parse(File.read(SOURCE)).fetch("glossary")
    end

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
