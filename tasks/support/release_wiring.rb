# frozen_string_literal: true

# Readers behind +rake gate:release:wiring+ (docs/releasing.md § Adding a
# crate to the linked group): the two seats a new crate can leave empty
# without anything saying so.
#
# +release-please+ reads the manifest as its record of what each package
# last released at, and rewrites a version only on a line carrying the
# +x-release-please-version+ annotation. A package missing from the
# manifest reads as never released; a generic extra-file missing its
# annotation is rewritten to nothing. Both report success.
module KobakoReleaseWiring
  module_function

  # Package paths declared in the config that the manifest does not
  # record. Such a package never joins the linked group's bump.
  def unrecorded_packages(config:, manifest:)
    packages(config).keys - manifest.keys
  end

  # +[[package path, file path], ...]+ for every +generic+ extra-file.
  # The generic updater is the only one driven by an in-file annotation,
  # so these are the files an annotation has to appear in; the others
  # name their target with a +jsonpath+ instead.
  def annotated_files(config)
    packages(config).flat_map do |package, settings|
      (settings["extra-files"] || []).filter_map do |entry|
        [package, entry["path"].to_s.sub(%r{\A/}, "")] if entry.is_a?(Hash) && entry["type"] == "generic"
      end
    end
  end

  # Violations across +files+, given as +[[package path, file path,
  # content], ...]+ with a +nil+ content for a path that does not exist:
  # a declared file that is absent, one carrying no annotation at all, or
  # an annotated version disagreeing with what the manifest records its
  # package at. A package the manifest does not record is
  # +unrecorded_packages+' to report, so it is skipped here.
  #
  # Every annotated line is checked, not just the first: the updater
  # rewrites them all to the releasing package's version, so a line
  # pinning a sibling is only correct while the two agree.
  def annotation_violations(files:, manifest:)
    files.flat_map do |package, path, content|
      expected = manifest[package]
      next [] if expected.nil?
      next ["#{path}: declared as a generic extra-file but absent"] if content.nil?

      found = annotated_versions(content)
      next ["#{path}: no x-release-please-version annotation"] if found.empty?

      found.reject { |version| version == expected }
           .map { |version| "#{path}: annotates #{version}, manifest records #{package} at #{expected}" }
    end
  end

  # The version on each annotated line, in file order.
  def annotated_versions(content)
    content.each_line
           .select { |line| line.include?("x-release-please-version") }
           .filter_map { |line| line[/\d+\.\d+\.\d+/] }
  end

  def packages(config)
    config["packages"] || {}
  end
end
