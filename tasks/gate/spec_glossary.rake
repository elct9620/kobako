# frozen_string_literal: true

# Glossary consistency gate: the ubiquitous language holds together on its own
# terms. A concept has exactly one name (N-6), and a name it rejects is one the
# corpus never uses — a rejection of a word still in use, or of a word that is
# itself one of our terms, would flag legitimate prose rather than the drift it
# was written to catch. Reader unit coverage rides test/tasks/test_spec_glossary.rb.
#
# Whether a word the corpus uses is a concept missing from the glossary is not
# asked here: `Fault` belongs and `Symbol` does not, and no lexical rule
# separates them. That gap is filled by reading, not by blocking a release.

require_relative "../support/spec/glossary"
require_relative "../support/report"

# The spec corpus N-6 governs — the same reach `gate:anchors` reads, minus the
# tests and benchmarks, which consume the vocabulary rather than define it.
GLOSSARY_CORPUS = FileList["SPEC.md", "docs/**/*.md"]

namespace :gate do
  namespace :spec do
    desc "Check the glossary declares each concept once and rejects only unused names."
    task :glossary do
      entries = KobakoSpec::Glossary.load
      sources = GLOSSARY_CORPUS.to_h { |path| [path, File.read(path)] }
      violations = KobakoSpec::Glossary.violations(entries, sources)

      puts KobakoReport.gate(name: "gate:spec:glossary",
                             ok_summary: "#{entries.size} concept(s), " \
                                         "#{KobakoSpec::Glossary.rejections(entries).size} rejected name(s)",
                             violations: violations, noun: "inconsistency")
    end
  end
end
