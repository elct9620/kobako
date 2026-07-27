# frozen_string_literal: true

# +rake gate:release:wiring+ — the two seats a crate can leave empty on the
# release track without anything saying so: a package the manifest does not
# record, and a generic extra-file carrying no version annotation. Both let
# a release report success while leaving a version where it was.
#
# The loud seats — the publish script, the workflow, the crates.io
# placeholder — announce themselves at release time and are enumerated
# beside these in docs/releasing.md, which the failure points at: a crate
# that missed one of these two usually missed those as well.
# Reader unit coverage rides test/tasks/test_release_wiring.rb.

require "json"

require_relative "../support/release_wiring"
require_relative "../support/report"

namespace :gate do
  namespace :release do
    desc "Check every package is manifest-recorded and every annotated version file is in sync."
    task :wiring do
      config = JSON.parse(File.read("release-please-config.json"))
      manifest = JSON.parse(File.read(".release-please-manifest.json"))

      files = KobakoReleaseWiring.annotated_files(config).map do |package, path|
        [package, path, (File.read(path) if File.exist?(path))]
      end

      unrecorded = KobakoReleaseWiring.unrecorded_packages(config: config, manifest: manifest)
                                      .map { |package| "#{package}: no .release-please-manifest.json entry" }

      puts KobakoReport.gate(
        name: "gate:release:wiring",
        ok_summary: "#{manifest.size} packages recorded, #{files.size} annotated version files in sync",
        violations: unrecorded + KobakoReleaseWiring.annotation_violations(files: files, manifest: manifest),
        noun: "unfilled seat",
        hint: "Every seat a crate needs is listed in docs/releasing.md § Adding a crate to the linked group."
      )
    end
  end
end
