# frozen_string_literal: true

require "minitest/autorun"

# Anti-revert guard: committed source must not cite `tmp/REFERENCE.md`.
#
# `tmp/REFERENCE.md` is a development scratch document — gitignored and
# never shipped. Citations to it from committed code are dead pointers
# that mislead anyone who clones the repo. Once the codebase is the
# Ground Truth, REFERENCE citations must stay out.
#
# This test scans the committed source tree for the forbidden tokens
# and fails immediately if any reappear. Future contributors (and
# SubAgents) get a loud, localized failure rather than a slow drift.
class TestNoReferenceCitations < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)

  # Directories whose contents are part of the shipped repo and should
  # be REFERENCE-free.
  SCAN_DIRS = %w[
    lib
    ext
    wasm/kobako-wasm/src
    tasks
    test
    build_config
    sig
  ].freeze

  # Top-level files to scan in addition to SCAN_DIRS.
  SCAN_FILES = %w[
    kobako.gemspec
    Rakefile
    README.md
    CHANGELOG.md
    Cargo.toml
    wasm/kobako-wasm/Cargo.toml
    wasm/kobako-wasm/build.rs
    ext/kobako/Cargo.toml
  ].freeze

  # Forbidden tokens. `tmp/REFERENCE` covers `tmp/REFERENCE.md` paths;
  # `REFERENCE.md` and `REFERENCE Ch.` cover prose citations.
  FORBIDDEN_PATTERNS = [
    "tmp/REFERENCE",
    "REFERENCE.md",
    "REFERENCE Ch."
  ].freeze

  # Paths excluded even if they live under SCAN_DIRS.
  EXCLUDED_FRAGMENTS = %w[tmp/ .powerloop/ .git/ vendor/ target/ node_modules/ SPEC.md].freeze

  def excluded_path?(rel_path)
    EXCLUDED_FRAGMENTS.any? { |frag| rel_path.include?(frag) }
  end

  def directory_files
    SCAN_DIRS.flat_map do |dir|
      base = File.join(PROJECT_ROOT, dir)
      next [] unless File.directory?(base)

      Dir.glob(File.join(base, "**", "*"), File::FNM_DOTMATCH)
         .reject { |p| File.directory?(p) }
         .reject { |p| excluded_path?(p.sub("#{PROJECT_ROOT}/", "")) }
    end
  end

  def top_level_files
    SCAN_FILES.map { |rel| File.join(PROJECT_ROOT, rel) }.select { |abs| File.file?(abs) }
  end

  def candidate_files
    files = directory_files + top_level_files
    self_path = File.expand_path(__FILE__)
    files.reject { |p| File.expand_path(p) == self_path }
  end

  def safe_read(path)
    content = File.read(path, mode: "rb").force_encoding("UTF-8")
    content.valid_encoding? ? content : nil
  rescue StandardError
    nil
  end

  def offenders_in(path, content)
    rel = path.sub("#{PROJECT_ROOT}/", "")
    matches = []
    content.each_line.with_index(1) do |line, lineno|
      next unless FORBIDDEN_PATTERNS.any? { |pat| line.include?(pat) }

      matches << "#{rel}:#{lineno}: #{line.strip}"
    end
    matches
  end

  def test_no_committed_file_cites_reference_md
    offenders = candidate_files.flat_map do |path|
      content = safe_read(path)
      content ? offenders_in(path, content) : []
    end

    assert_empty offenders,
                 "Committed code must not cite tmp/REFERENCE.md — the codebase " \
                 "is the Ground Truth. Replace REFERENCE citations with a " \
                 "precise SPEC.md cite, or delete them. Offenders:\n" \
                 "#{offenders.join("\n")}"
  end
end
