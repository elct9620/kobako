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
  # [<tt>Bind.new(path, exclusive: false)</tt>]
  #   A host directory and everything beneath it. Nothing outside it is
  #   reachable, a symbolic link pointing out included. With +exclusive+
  #   the Host App vouches that only the guest changes the directory, so a
  #   name the guest found is not looked up on the host again; a change the
  #   host makes meanwhile may go unseen.
  # [<tt>Overlay.new(lower: [filesystem, ...], upper: tmpfs)</tt>]
  #   Read-only lower Layers, topmost first, under an upper Tmpfs that
  #   takes every change, as Linux's overlayfs stacks them. Without
  #   +upper+ the view is read-only and needs two lower Layers. It takes
  #   at most 500 lower Layers and stacks at most two Overlays deep.
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
  #
  # Each method beyond +open+ and +stat+ is optional, and a change it lacks
  # raises <tt>Errno::EROFS</tt> in the guest:
  #
  # [<tt>children(path)</tt>]
  #   Entry names, as Dir.children gives them.
  # [<tt>mkdir(path)</tt>, <tt>rmdir(path)</tt>, <tt>unlink(path)</tt>, <tt>rename(from, to)</tt>]
  #   The changes a writable mount passes on.
  #
  # A +stat+ may also answer +dev+, else the Filesystem stands for one
  # device, and +ftype+, which says a FIFO, a socket, or a device is one
  # so the guest's open is refused without asking +open+.
  #
  # An object whose +exclusive?+ answers true vouches that only the guest
  # changes it, so +stat+ is asked about a name once rather than again on
  # each walk, as <tt>Bind.new(path, exclusive: true)</tt> does.
  #
  # The file +open+ answers takes the guest's reads and writes, each named
  # as +IO+ names it:
  #
  # [<tt>pread(length, offset)</tt>, +size+]
  #   Required: the bytes there, +nil+ or EOFError at the end, and the
  #   length.
  # [<tt>pwrite(data, offset)</tt>, <tt>truncate(length)</tt>]
  #   The bytes written, and nothing; without them a write raises
  #   <tt>Errno::EROFS</tt>.
  # [<tt>append(data)</tt>]
  #   Writes all of +data+ at the end in one step, so another writer's
  #   append never lands over it, and answers where the file then ends;
  #   without it an append raises <tt>Errno::ENOTSUP</tt>.
  # [+fsync+, +close+]
  #   Nothing; without them there is nothing to flush or release.
  # [+stat+]
  #   What <tt>stat(path)</tt> answers, for the file's times and count of
  #   names; without it the file has one name and no time.
  module Filesystem
  end
end
