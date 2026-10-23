# frozen_string_literal: true

module Kobako
  # The Filesystems a Sandbox can mount for the guest's +File+, each
  # answered in Rust by the kind of the same name. Hand one to
  # Sandbox#mount:
  #
  #   sandbox.mount("/data", Kobako::Filesystem::Memory.new("config.yml" => yaml))
  #
  # [<tt>Memory.new(files = {}, limit: 64 MiB, inodes:)</tt>]
  #   Files held in memory for as long as the object lives, seeded from a
  #   Hash of path to contents. Every invocation shares them.
  # [<tt>Temporary.new(limit: 64 MiB, inodes:)</tt>]
  #   Files held in memory that every invocation starts empty.
  # [<tt>Directory.new(path)</tt>]
  #   A host directory and everything beneath it. Nothing outside it is
  #   reachable, a symbolic link pointing out included.
  # [<tt>Overlay.new.layer(filesystem, access: :read)</tt>]
  #   Layers stacked into one view, the first added on top, each +:read+
  #   or +:read_write+ on its own. A change lands in a writable Layer, and
  #   a read-only one stays as it was. Each +layer+ answers a new Overlay.
  #
  # A Memory or Temporary is bounded as a tmpfs mount is: its files hold
  # at most +limit+ bytes together, and it holds at most +inodes+ files and
  # directories, its root among them; +nil+ lifts either bound. Without
  # +inodes+ it holds one for each 4 KiB of +limit+, and the root. Past
  # either, the guest raises Errno::ENOSPC.
  #
  # Any object answering +open+ and +stat+ mounts too, as a Filesystem
  # written in Ruby; its methods take the names of +File+, +Dir+, and +IO+.
  module Filesystem
  end
end
