#!/usr/bin/env ruby
# frozen_string_literal: true

# VFS overlay demo: the guest's own `File` over a host directory that it can
# read, and "edit", while the files on the host stay untouched.
#
# The lesson this example encodes
# -------------------------------
# `Sandbox#mount` places a Filesystem at a guest path, where the guest's
# `File` reaches it. An Overlay stacks Layers into one view, the first added
# on top:
#
#   File.write("/disk/sample.txt", ...) -> copied up into the Temporary Layer
#   File.read("/disk/sample.txt")       -> the Temporary copy, else the Directory
#
# The Temporary Layer starts empty at every invocation, so two things follow,
# and the run below shows both:
#
#   * within one invocation, a write is visible to a later read — the guest
#     sees its own changed result;
#   * across invocations the change is gone, and the Directory Layer is
#     read-only, so the real file on the host is never touched.
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
  gem "kobako", "~> 0.29.0"
end

require "kobako"

# Example scripts are nested under Vfs so the file carries a single
# top-level constant and reads top-down.
module Vfs
  # First invocation: read the original, overwrite it, read it back. The
  # second read hits the Temporary Layer, so the guest observes its own write.
  WRITE_THEN_READ = <<~MRUBY
    before = File.read("/disk/sample.txt")
    File.write("/disk/sample.txt", "patched in memory\\n")
    after = File.read("/disk/sample.txt")
    { "name" => File.basename("/disk/sample.txt"), "before" => before, "after" => after }
  MRUBY

  # Second invocation: read only. The Temporary Layer is empty again, so the
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

  # A writable Temporary Layer over the host directory, read-only.
  def self.overlay(root)
    Kobako::Filesystem::Overlay.new
                               .layer(Kobako::Filesystem::Temporary.new, access: :read_write) # empty every run
                               .layer(Kobako::Filesystem::Directory.new(root)) # never written
  end
end

disk_root = __dir__
sample_path = File.join(disk_root, "sample.txt")

sandbox = Kobako::Sandbox.new
sandbox.mount("/disk", Vfs.overlay(disk_root), access: :read_write, &Vfs::ALLOW)

first = sandbox.eval(Vfs::WRITE_THEN_READ).value
second = sandbox.eval(Vfs::READ_ONLY).value
refusals = sandbox.eval(Vfs::REFUSALS).value

disk_after = File.read(sample_path)

puts "vfs overlay demo — the guest's File over a host directory it cannot change"
puts
puts "invocation 1 — write then read (one Temporary Layer):"
puts "  read  before write : #{first["before"].inspect}   # the host directory"
puts "  write \"patched in memory\\n\"                       # copied up into the Temporary Layer"
puts "  read  after  write : #{first["after"].inspect}  # the changed view"
puts "  File.basename ran in the guest: #{first["name"].inspect}"
puts
puts "invocation 2 — a fresh Temporary Layer, read only:"
puts "  read               : #{second.inspect}   # the host directory again"
puts
puts "refusals — what the mount does not grant:"
refusals.each { |name, answer| puts "  #{name.ljust(16)}: #{answer}" }
puts
puts "sample.txt on the host (after every run):"
puts "  #{disk_after.inspect}   # never changed"
