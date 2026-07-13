# frozen_string_literal: true

require "yaml"

# Consistency comparator backing +rake gate:rbs:lock+. Gems whose RBS ships in
# the gem itself (+source.type: rubygems+ in +rbs_collection.lock.yaml+)
# carry a concrete version that +rbs collection update+ copies from
# Gemfile.lock; a mismatch means the lock was not regenerated after a
# dependency bump, so steep silently loads a fallback definition. Git
# collection and stdlib sources keep their own RBS versioning and are out
# of scope.
module KobakoRbsLock
  module_function

  # +[[name, rbs_version, gemfile_version], ...]+ for every gem-sourced RBS
  # pin whose version disagrees with Gemfile.lock; +gemfile_version+ is
  # +nil+ when the gem is absent from Gemfile.lock entirely. An empty
  # result means the collection lock is in sync.
  def drift(collection_yaml:, gemfile_lock:)
    locked = gemfile_lock_versions(gemfile_lock)
    gem_sourced(collection_yaml).filter_map do |name, version|
      actual = locked[name]
      [name, version, actual] unless actual == version
    end
  end

  # +[[name, version], ...]+ for the gems whose RBS source is the published
  # gem, the only entries whose version tracks Gemfile.lock.
  def gem_sourced(collection_yaml)
    data = YAML.safe_load(collection_yaml) || {}
    (data["gems"] || []).filter_map do |gem|
      [gem["name"], gem["version"].to_s] if gem.dig("source", "type") == "rubygems"
    end
  end

  # Gem name → version string parsed from the Gemfile.lock +specs+ listing
  # (four-space-indented +name (version)+ lines).
  def gemfile_lock_versions(gemfile_lock)
    gemfile_lock.each_line.filter_map do |line|
      match = line.match(/^ {4}([a-zA-Z0-9._-]+) \(([^()]+)\)$/)
      [match[1], match[2]] if match
    end.to_h
  end
end
