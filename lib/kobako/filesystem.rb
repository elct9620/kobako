# frozen_string_literal: true

module Kobako
  # The Filesystems a Sandbox can mount for the guest's +File+, each
  # answered in Rust by the kind of the same name. Hand one to
  # Sandbox#mount:
  #
  #   sandbox.mount("/data", Kobako::Filesystem::Tmpfs.new("config.yml" => yaml))
  #
  # [<tt>Tmpfs.new(files = {}, size: 64 MiB, nr_inodes:)</tt>]
  #   Files held in memory for as long as the object lives, seeded from a
  #   Hash of path to contents. Every invocation shares them; one that
  #   should start empty mounts a new Tmpfs through Context#mount.
  # [<tt>Bind.new(path)</tt>]
  #   A host directory and everything beneath it. Nothing outside it is
  #   reachable, a symbolic link pointing out included.
  # [<tt>Overlay.new(lower: [filesystem, ...], upper: tmpfs)</tt>]
  #   Read-only lower Layers, topmost first, under an upper Tmpfs that
  #   takes every change, as Linux's overlayfs stacks them. Without
  #   +upper+ the view is read-only and needs two lower Layers.
  #
  # A Tmpfs is bounded as a tmpfs mount is: its files hold
  # at most +size+ bytes together, and it holds at most +nr_inodes+ files and
  # directories, its root among them; +nil+ lifts either bound. Without
  # +nr_inodes+ it holds one for each 4 KiB of +size+, and the root. Past
  # either, the guest raises Errno::ENOSPC.
  #
  # Any object answering +open+ and +stat+ mounts too, as a Filesystem
  # written in Ruby; its methods take the names of +File+, +Dir+, and +IO+.
  # Its +stat+ answers +directory?+, +size+, and +ino+, an +ino+ naming one
  # directory for as long as the guest holds it open. Its names match
  # exactly, as a mount's block judges them as given. A name may hold a
  # <tt>\\</tt>, which the guest's paths never separate on, so one joined
  # to a Windows host path is the Filesystem's to refuse.
  module Filesystem
  end
end
