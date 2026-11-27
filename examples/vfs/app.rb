#!/usr/bin/env ruby
# frozen_string_literal: true

# VFS overlay demo: the guest's own `File` over a host directory that it can
# read, and "edit", while the files on the host stay untouched.
#
# The lesson this example encodes
# -------------------------------
# `Sandbox#mount` places a Filesystem at a guest path, where the guest's
# `File` reaches it. An Overlay stacks a writable Tmpfs over read-only lower
# Layers, as Linux's overlayfs does:
#
#   File.write("/disk/sample.txt", ...) -> copied up into the upper Tmpfs
#   File.read("/disk/sample.txt")       -> the upper copy, else the Bind below
#
# Every invocation reaches the same Filesystem a Sandbox mounts, so the
# Sandbox mounts the host directory read-only, and each run mounts an
# Overlay over a Tmpfs of its own through `ctx.mount`. Two things follow,
# and the run below shows both:
#
#   * within one invocation, a write is visible to a later read — the guest
#     sees its own changed result;
#   * the next invocation gets a new Tmpfs, so the change is gone, and the
#     Bind is never written, so the real file on the host is never touched.
#
# The guest also chooses the path, so the mount answers each open and change
# through a block: here it lets the guest read anything beneath /disk but
# change only sample.txt. A path that climbs out of the mount is refused, and
# a path nothing is mounted at does not exist. Mount the least authority the
# guest needs, never the whole filesystem.
#
# Usage:
#   ruby examples/vfs/app.rb
#
# From a clone of the kobako repository, prefix with `bundle exec` so the
# local checkout is used instead of the released gem.

require "bundler/inline"

gemfile do
  source "https://rubygems.org"
  gem "kobako", "~> 0.32.0"
end

require "kobako"

# Example scripts are nested under Vfs so the file carries a single
# top-level constant and reads top-down.
module Vfs
  # First invocation: read the original, overwrite it, read it back. The
  # second read hits the upper Tmpfs, so the guest observes its own write.
  WRITE_THEN_READ = <<~MRUBY
    before = File.read("/disk/sample.txt")
    File.write("/disk/sample.txt", "patched in memory\\n")
    after = File.read("/disk/sample.txt")
    { "name" => File.basename("/disk/sample.txt"), "before" => before, "after" => after }
  MRUBY

  # Second invocation: read only. Its upper Tmpfs is new and empty, so the
  # read falls through to the host directory.
  READ_ONLY = <<~MRUBY
    File.read("/disk/sample.txt")
  MRUBY

  # Each refusal the guest meets, by the Errno class it raises.
  REFUSALS = <<~MRUBY
    {
      "write app.rb"     => -> { File.write("/disk/app.rb", "x") },
      "climb out"        => -> { File.read("/disk/../../etc/passwd") },
      "outside a mount"  => -> { File.read("/etc/passwd") }
    }.map do |name, call|
      begin
        call.call
        [name, "NOT refused"]
      rescue SystemCallError => e
        [name, e.class.to_s]
      end
    end
  MRUBY

  # The guest reads anything beneath the mount and changes only sample.txt.
  ALLOW = ->(path, access) { access == :read || path == "/disk/sample.txt" }

  # A writable Tmpfs, new for this run, over the host directory, read-only.
  def self.overlay(bind)
    Kobako::Filesystem::Overlay.new(
      lower: [bind],                       # never written
      upper: Kobako::Filesystem::Tmpfs.new # takes every change, this run only
    )
  end
end

disk_root = __dir__
sample_path = File.join(disk_root, "sample.txt")
disk = Kobako::Filesystem::Bind.new(disk_root)

sandbox = Kobako::Sandbox.new
sandbox.mount("/disk", disk, &Vfs::ALLOW) # read-only unless a run mounts its own

results = [Vfs::WRITE_THEN_READ, Vfs::READ_ONLY, Vfs::REFUSALS].map do |source|
  sandbox.eval(source) { |ctx| ctx.mount("/disk", Vfs.overlay(disk), access: :read_write, &Vfs::ALLOW) }.value
end
first, second, refusals = results

disk_after = File.read(sample_path)

puts "vfs overlay demo — the guest's File over a host directory it cannot change"
puts
puts "invocation 1 — write then read (its own Tmpfs):"
puts "  read  before write : #{first["before"].inspect}   # the host directory"
puts "  write \"patched in memory\\n\"                       # copied up into the Tmpfs"
puts "  read  after  write : #{first["after"].inspect}  # the changed view"
puts "  File.basename ran in the guest: #{first["name"].inspect}"
puts
puts "invocation 2 — a new Tmpfs, read only:"
puts "  read               : #{second.inspect}   # the host directory again"
puts
puts "refusals — what the mount does not grant:"
refusals.each { |name, answer| puts "  #{name.ljust(16)}: #{answer}" }
puts
puts "sample.txt on the host (after every run):"
puts "  #{disk_after.inspect}   # never changed"
