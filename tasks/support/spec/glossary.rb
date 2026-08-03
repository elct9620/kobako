# frozen_string_literal: true

require "yaml"

module KobakoSpec
  # Reader and renderer behind +rake spec:generate+ and +rake gate:spec:glossary+:
  # the ubiquitous language every other specification is written in.
  #
  # The glossary is the layer every other spec is written in, so it names
  # concepts and nothing below them — no Ruby or Rust spelling, no anchor. That
  # keeps what is checkable here purely structural: a concept cannot be told
  # from a common technical term by any lexical rule, so "is this word missing
  # from the glossary" stays a human reading rather than a gate.
  #
  # What a gate can settle is the negative space. A rejected name is only
  # enforceable when the corpus never uses it in another sense, which makes the
  # zero-occurrence count both the admission test for a +not+ entry and the
  # check that keeps it true. The rendered glossary is excluded from that count:
  # the page that declares a rejection is not a use of it.
  module Glossary
    # Where the concepts are declared.
    SOURCE = "docs/spec/_data/glossary.yml"

    # The rendered page, excluded from the banned-word scan.
    OUTPUT = "docs/spec/glossary.md"

    # The prose the renderer wraps the tables in. The page states what a
    # reader may do with it, since it carries no anchors to follow.
    PREAMBLE = [
      "kobako's ubiquitous language: one concept, one name. Every other " \
      "specification is written in these words.",
      "",
      "Concepts only. How a concept is spelled in Ruby or Rust belongs to the " \
      "interface spec, and what it does to the behavior spec — both are " \
      "written in this vocabulary rather than referenced from it.",
      "",
      "Generated from `_data/glossary.yml` by `rake spec:generate`; edit that file."
    ].freeze

    module_function

    # The declared entries, in file order — the order the page renders in.
    def load
      YAML.safe_load_file(SOURCE)
    end

    # +entries+ as the glossary page: the terms, then the names they reject.
    # The rejection table is omitted when nothing is rejected yet, so an empty
    # section never reads as a complete one.
    def render(entries)
      sections = ["# Glossary", "", *PREAMBLE, "", *terms_table(entries)]
      rejected = rejections(entries)
      sections += ["", "## Rejected names", "", *rejected_table(rejected)] unless rejected.empty?
      "#{sections.join("\n")}\n"
    end

    # Every way +entries+ can be wrong on its own terms, as reader-facing
    # lines. +sources+ is the corpus as +{path => text}+.
    def violations(entries, sources)
      duplicates(entries) + self_rejections(entries) + live_rejections(entries, sources)
    end

    # +[term, word, why]+ for each rejected name, in entry order.
    def rejections(entries)
      entries.flat_map do |entry|
        (entry["not"] || []).map { |rejection| [entry["term"], rejection["word"], rejection["why"]] }
      end
    end

    # Terms declared more than once (N-6: a concept has exactly one name, so
    # a name answers to exactly one concept).
    def duplicates(entries)
      entries.map { |entry| entry["term"] }
             .tally.select { |_, count| count > 1 }
                   .map { |term, count| "#{term}: declared #{count} times" }
    end
    private_class_method :duplicates

    # Rejected names that are themselves declared terms. Both words being ours
    # means the reader tells them apart from the definitions; scanning for one
    # would flag every legitimate use of the other.
    def self_rejections(entries)
      terms = entries.map { |entry| entry["term"] }
      rejections(entries).filter_map do |term, word, _|
        "#{term}: rejects #{word.inspect}, which is a declared term" if terms.include?(word)
      end
    end
    private_class_method :self_rejections

    # Rejected names the corpus still uses. A word in use cannot be rejected —
    # the scan cannot tell a misuse from the sense the corpus already relies on.
    def live_rejections(entries, sources)
      rejections(entries).flat_map do |term, word, _|
        occurrences(word, sources).map { |path| "#{term}: rejects #{word.inspect}, still used in #{path}" }
      end
    end
    private_class_method :live_rejections

    # The corpus paths using +word+ as a whole word, case-sensitively.
    def occurrences(word, sources)
      pattern = /(?<![A-Za-z0-9_])#{Regexp.escape(word)}(?![A-Za-z0-9_])/
      sources.reject { |path, _| path == OUTPUT }
             .select { |_, text| text.match?(pattern) }
             .keys
    end
    private_class_method :occurrences

    def terms_table(entries)
      rows = entries.map { |entry| "| **#{entry["term"]}** | #{entry["definition"].strip} |" }
      ["| Term | Definition |", "|------|------------|", *rows]
    end
    private_class_method :terms_table

    def rejected_table(rejected)
      rows = rejected.map { |term, word, why| "| #{term} | `#{word}` | #{why.strip} |" }
      ["| Term | Not | Why |", "|------|-----|-----|", *rows]
    end
    private_class_method :rejected_table
  end
end
