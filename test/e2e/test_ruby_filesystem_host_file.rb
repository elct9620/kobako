# frozen_string_literal: true

require "test_helper"
require "tmpdir"

# E2E — a Ruby Filesystem handing each open on to a real ::File in a host
# directory, reached through the real Guest Binary's File.
class TestE2ERubyFilesystemHostFile < Minitest::Test
  include E2eGuestHelper

  # Files a host directory holds, each opened as a real ::File.
  class HostFilesystem
    def initialize(root) = @root = root
    def open(path, flags) = File.open(File.join(@root, path), flags)
    def stat(path) = File.stat(File.join(@root, path))
  end

  # The host follows a link a real ::File opens unless told not to, so the
  # link's target holds what a followed open would read.
  # @behavior RF-011
  def test_a_ruby_filesystems_open_is_told_to_follow_no_link
    Dir.mktmpdir do |root|
      File.write(File.join(root, "target"), "reached")
      File.symlink("target", File.join(root, "link"))
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", HostFilesystem.new(root))
      read = 'begin; File.read("/data/link"); rescue Errno::ELOOP; :refused; end'

      assert_equal :refused, sandbox.eval(read).value,
                   "File.read of a host link through a Sandbox#mount of a Ruby Filesystem whose open " \
                   "answers a real ::File must be refused as a link, its target unread"
    end
  end

  # A real ::File raises EOFError for a pread at its end, which a
  # Filesystem answering nil there never exercises.
  # @behavior RF-013
  def test_a_real_file_a_ruby_filesystem_opens_reads_to_its_end
    Dir.mktmpdir do |root|
      File.write(File.join(root, "note.txt"), "hello")
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", HostFilesystem.new(root))

      assert_equal "hello", sandbox.eval('File.read("/data/note.txt")').value,
                   "File.read through a Sandbox#mount of a Ruby Filesystem whose open answers a real ::File " \
                   "must read the whole file"
    end
  end
end
