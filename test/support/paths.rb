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
  # a directory still open, so the collector runs first. One collection need
  # not reach it, as Ruby's collector keeps whatever the machine stack still
  # appears to point at, so a refusal asks again a few times. A last refusal
  # says what Ruby still keeps: none of these alive while the directory stays
  # open means something beneath Ruby holds it.
  def remove_host_directory(dir, attempts: 5)
    tries ||= 0
    tries += 1
    GC.start
    FileUtils.remove_entry(dir)
  rescue Errno::EACCES => e
    retry if tries < attempts
    raise e.exception("#{e.message} (after #{tries} collections, alive: #{host_directory_holders})")
  end

  # How many of each object that can keep a host directory open are alive.
  def host_directory_holders
    [Kobako::Filesystem::Bind, Kobako::Filesystem::Overlay, Kobako::Sandbox, Kobako::Runtime]
      .map { |holder| "#{holder.name.split("::").last}=#{ObjectSpace.each_object(holder).count}" }
      .join(" ")
  end
end
