# frozen_string_literal: true

require "fileutils"
require "tmpdir"

# Repository-root-anchored path resolver for the test suite. Test files sit
# at varying depths under +test/+, so reaching a fixture or a build artifact
# by +__dir__+-relative traversal is brittle — moving a file silently breaks
# every path it computes. TestPaths anchors on the repository root (this
# file's own +test/support/+ location is fixed), so a resolved path stays
# correct wherever the caller lives.
module TestPaths
  ROOT = File.expand_path("../..", __dir__)

  module_function

  # A path relative to the repository root — the escape hatch for crate
  # manifests and source trees (+crates/+, +wasm/+).
  def source(*segments)
    File.expand_path(File.join(ROOT, *segments))
  end

  # A built Guest Binary or other artifact under +data/+.
  def data(name)
    source("data", name)
  end

  # A test fixture under +test/fixtures/+.
  def fixture(name)
    source("test", "fixtures", name)
  end

  # A host directory a test mounts, removed once the block returns.
  def host_directory
    dir = Dir.mktmpdir("kobako")
    yield dir
  ensure
    remove_host_directory(dir) if dir
  end

  # Remove +dir+ once nothing mounts it. A Bind holds the directories it
  # opened until its Filesystem is collected, and Windows refuses to remove
  # a directory still open, so the collector runs first.
  def remove_host_directory(dir)
    GC.start
    FileUtils.remove_entry(dir)
  end
end
