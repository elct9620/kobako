# frozen_string_literal: true

# Generated-spec drift gate: every page projected from `docs/spec/_data/` still
# matches what its data file renders to. A page under `docs/spec/` is wholly
# hand-written or wholly generated, so an edit made to a generated page is a
# fact recorded in the wrong place — this is what makes that visible instead of
# silently lost at the next `rake spec:generate`.
#
# The comparison runs in memory rather than by regenerating and diffing, so a
# gate run leaves the working tree alone and reports the same drift whether or
# not the page is committed.

require_relative "../support/spec/glossary"
require_relative "../support/report"

namespace :gate do
  namespace :spec do
    desc "Check every generated spec page matches its docs/spec/_data/ source."
    task :generated do
      page = KobakoSpec::Glossary::OUTPUT
      rendered = KobakoSpec::Glossary.render(KobakoSpec::Glossary.load)
      stale = File.exist?(page) && File.read(page) == rendered ? [] : ["#{page}: differs from its data file"]

      puts KobakoReport.gate(name: "gate:spec:generated",
                             ok_summary: "#{page} matches its data file",
                             violations: stale, noun: "stale page",
                             hint: "Run `rake spec:generate` and commit the result.")
    end
  end
end
