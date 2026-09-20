# frozen_string_literal: true

# Consistency comparator backing +rake gate:readme:pins+. A crate README's
# install snippet is what a consumer copies, so a third-party dependency it
# names has to carry the release series the manifest beside it depends on —
# a snippet naming an older series hands out a build that will not compile
# against the crate it describes. Versions of this repository's own crates
# carry +# x-release-please-version+ and move with the release tooling, so
# they stay out of scope; a snippet line naming a dependency the manifest
# does not have is a consumer's own to add and is left alone.
module KobakoReadmePins
  module_function

  # +[[name, snippet_version, manifest_version], ...]+ for every dependency
  # a README's TOML snippets pin at a series its manifest does not carry.
  # An empty result means the snippets are in sync.
  def drift(readme:, manifest:)
    declared = manifest_versions(manifest)
    snippet_pins(readme).filter_map do |name, version|
      actual = declared[name]
      [name, version, actual] if actual && !series?(version, actual)
    end
  end

  # +[[name, version], ...]+ for the plain +name = "version"+ lines inside a
  # README's TOML fences, skipping the ones the release tooling owns.
  def snippet_pins(readme)
    toml_fences(readme).flat_map do |fence|
      fence.each_line.filter_map do |line|
        next if line.include?("x-release-please-version")

        match = line.match(/^([a-zA-Z0-9_-]+) = "([^"]+)"/)
        [match[1], match[2]] if match
      end
    end
  end

  # Dependency name → version string, reading the bare +name = "version"+ and
  # the +name = { version = "..." }+ table form alike.
  def manifest_versions(manifest)
    manifest.each_line.filter_map do |line|
      match = line.match(/^([a-zA-Z0-9_-]+) = (?:"([^"]+)"|\{[^}]*version = "([^"]+)")/)
      [match[1], match[2] || match[3]] if match
    end.to_h
  end

  # Whether +snippet+ names the release series +manifest+ depends on. A
  # snippet may shorten +0.18.0+ to +0.18+; it may never name another series.
  def series?(snippet, manifest)
    manifest == snippet || manifest.start_with?("#{snippet}.")
  end

  # The bodies of a README's +toml+ fences, where an install snippet lives.
  def toml_fences(readme)
    readme.scan(/^```toml\n(.*?)^```/m).flatten
  end
end
