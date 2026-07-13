#!/usr/bin/env ruby
# frozen_string_literal: true

# VFS overlay demo: a native-style `File` for the guest, backed by a host
# overlay that reads through to the real disk but intercepts every write
# into memory — so untrusted guest code can freely "edit" files while the
# disk stays pristine.
#
# The lesson this example encodes
# -------------------------------
# `Kobako::Extension` teaches the guest a native-style constant. Here the
# `File` idiom's I/O methods are NOT defined in-guest, so they dispatch to
# a bound host backend — an `OverlayFileSystem` — under the same isolation
# and reflection guarantees as any Service:
#
#   File.read(path)         -> host: overlay hit? serve it : read real disk
#   File.write(path, data)  -> host: store in the in-memory overlay only
#
# The backend is supplied through a CALLABLE provider, so `install`
# resolves a FRESH overlay at the start of every invocation. Two things
# follow, and the run below shows both:
#
#   * within one invocation, a write is visible to a later read — the
#     guest sees its own changed result;
#   * across invocations the overlay resets, so a write can never leak
#     into the next call, and the real file on disk is never touched.
#
# A read-through backend also crosses a trust boundary: the guest chooses
# the path. `OverlayFileSystem` therefore contains every read within its
# root, so `File.read("../../etc/passwd")` cannot escape the example
# directory — bind the least authority the guest needs, never the whole
# filesystem.
#
# Usage:
#   ruby examples/vfs/app.rb
#
# From a clone of the kobako repository, prefix with `bundle exec` so the
# local checkout is used instead of the released gem.

require "bundler/inline"

gemfile do
  source "https://rubygems.org"
  gem "kobako", "~> 0.16.0"
end

require "kobako"

# Example types are nested under Vfs so the file carries a single
# top-level constant and reads top-down.
module Vfs
  # The guest idiom: a native-style `File`. Path arithmetic like
  # `basename` runs in-guest with no round-trip; `read` / `write` are
  # deliberately left undefined, so they fall through to the bound host
  # backend.
  FILE_SOURCE = <<~MRUBY
    class File < Kobako::Member
      def self.basename(path) = path.split("/").last || ""
    end
  MRUBY

  # First invocation: read the original, overwrite it, read it back. The
  # second read hits the overlay, so the guest observes its own write.
  WRITE_THEN_READ = <<~'MRUBY'
    before = File.read("sample.txt")
    File.write("sample.txt", "patched in memory\n")
    after  = File.read("sample.txt")
    { "name" => File.basename("dir/sample.txt"), "before" => before, "after" => after }
  MRUBY

  # Second invocation: read only. The overlay was rebuilt for this call,
  # so the earlier write is gone and the read falls through to disk again.
  READ_ONLY = <<~MRUBY
    File.read("sample.txt")
  MRUBY

  # A traversal attempt. The host backend contains every path within the
  # overlay root, so this is refused before any disk read and the guest
  # sees a capability failure — the point of the example, exercised.
  ESCAPE_ATTEMPT = <<~MRUBY
    File.read("../../etc/passwd")
  MRUBY

  # Host backend bound at the guest's `File`. Reads fall through to the
  # real disk; writes are captured in an in-memory overlay that shadows
  # the disk for later reads without ever mutating it.
  class OverlayFileSystem
    def initialize(disk_root)
      @disk_root = ::File.expand_path(disk_root)
      @overlay = {}
    end

    # Read-through with copy-on-write shadowing: a path that was written
    # is served from the overlay; everything else is read from disk. The
    # resolved absolute path is the overlay key, so path aliases like
    # `sample.txt` and `./sample.txt` address the same shadowed entry.
    def read(path)
      key = resolve(path)
      return @overlay[key] if @overlay.key?(key)

      ::File.read(key)
    end

    # Writes land in the overlay only and never reach the disk. Keyed by
    # the same resolved path as `read`, so a write is visible to a later
    # read of any alias of that path — and contained to the root, so a
    # traversal cannot even shadow a file outside it. Returns the byte
    # count, matching Ruby's `File.write`.
    def write(path, content)
      @overlay[resolve(path)] = content
      content.bytesize
    end

    private

    # Contain every path within the overlay root so a guest-chosen path
    # cannot escape it (absolute paths and `..` traversal included), and
    # normalise aliases to one absolute key shared by reads and writes.
    def resolve(path)
      full = ::File.expand_path(path, @disk_root)
      return full if full == @disk_root || full.start_with?("#{@disk_root}/")

      raise ArgumentError, "path escapes the overlay root: #{path.inspect}"
    end
  end

  # Composes the guest idiom with the host backend. The provider is a
  # callable, so a fresh OverlayFileSystem backs the `File` path on every
  # invocation — writes cannot leak from one call into the next.
  def self.build_extension(disk_root)
    Kobako::Extension.new(
      name: :File,
      source: FILE_SOURCE,
      backend: Kobako::Extension::Backend.new(
        path: "File",
        provider: -> { OverlayFileSystem.new(disk_root) }
      )
    )
  end
end

disk_root = __dir__
sample_path = File.join(disk_root, "sample.txt")

sandbox = Kobako::Sandbox.new
sandbox.install(Vfs.build_extension(disk_root))

first = sandbox.eval(Vfs::WRITE_THEN_READ)
second = sandbox.eval(Vfs::READ_ONLY)

# Exercise the guard so running the example proves it: a traversal must be
# refused by the host backend, surfacing as a capability failure.
escape =
  begin
    sandbox.eval(Vfs::ESCAPE_ATTEMPT)
    "NOT rejected — the overlay guard is broken!"
  rescue Kobako::ServiceError
    "rejected"
  end

disk_after = File.read(sample_path)

puts "vfs overlay demo — a read-through overlay that protects the disk"
puts
puts "invocation 1 — write then read (one overlay):"
puts "  read  before write : #{first["before"].inspect}   # read-through to disk"
puts "  write \"patched in memory\\n\"                       # intercepted into overlay"
puts "  read  after  write : #{first["after"].inspect}  # overlay hit — the changed view"
puts "  basename ran in-guest, no round-trip: #{first["name"].inspect}"
puts
puts "invocation 2 — fresh overlay, read only:"
puts "  read               : #{second.inspect}   # overlay reset; disk view again"
puts
puts "escape attempt — read outside the overlay root:"
puts "  File.read(\"../../etc/passwd\") : #{escape}   # contained by the host backend"
puts
puts "sample.txt on disk (after both runs):"
puts "  #{disk_after.inspect}   # never mutated"
