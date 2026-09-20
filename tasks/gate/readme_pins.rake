# frozen_string_literal: true

# +rake gate:readme:pins+ — gate that each crate README's install snippet
# pins the same release series its manifest depends on, so the snippet a
# consumer copies builds against the crate it describes. The repository's
# own crate versions ride the release tooling's +x-release-please-version+
# markers; this covers the third-party pins beside them, which nothing else
# moves. Its comparator rides the tooling suite
# (+test/tasks/test_readme_pins.rb+).

require_relative "../support/readme_pins"
require_relative "../support/report"

namespace :gate do
  namespace :readme do
    desc "Verify each crate README's install snippet pins the series its manifest depends on."
    task :pins do
      readmes = Dir.glob("{crates,wasm}/*/README.md")
      pinned = 0

      violations = readmes.flat_map do |readme|
        manifest = File.join(File.dirname(readme), "Cargo.toml")
        next [] unless File.exist?(manifest)

        source = File.read(readme)
        pinned += KobakoReadmePins.snippet_pins(source).size
        KobakoReadmePins.drift(readme: source, manifest: File.read(manifest))
                        .map { |name, snippet, actual| "  #{readme}: #{name} #{snippet}, manifest #{actual}" }
      end

      puts KobakoReport.gate(
        name: "gate:readme:pins",
        ok_summary: "#{pinned} snippet pin(s) across #{readmes.size} crate README(s) match their manifests",
        violations: violations,
        noun: "drifted pin"
      )
    end
  end
end
