# frozen_string_literal: true

# Renders the spec pages that are projections of `docs/spec/_data/`. A page
# under `docs/spec/` is either wholly hand-written or wholly generated, so an
# edit to a generated page is lost by design — the data file is where the fact
# lives. `gate:spec:generated` holds the two in step.

require_relative "../support/spec/glossary"

namespace :spec do
  desc "Render every generated spec page from docs/spec/_data/."
  task :generate do
    File.write(KobakoSpec::Glossary::OUTPUT, KobakoSpec::Glossary.render(KobakoSpec::Glossary.load))
    puts "spec:generate: #{KobakoSpec::Glossary::OUTPUT}"
  end
end
