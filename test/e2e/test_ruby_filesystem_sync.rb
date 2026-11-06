# frozen_string_literal: true

require "test_helper"

# E2E — a script syncing a file and the directory a Ruby Filesystem opened.
class TestE2ERubyFilesystemSync < Minitest::Test
  include E2eGuestHelper

  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # One note, its file opened as +file_class+.
  class NoteFilesystem
    def initialize(file_class) = @file_class = file_class

    def open(_path, _flags) = @file_class.new
    def stat(path) = Stat.new(directory: path.empty?, size: 5, ino: path.empty? ? 1 : 2)
  end

  # A file answering the read subset and no fsync.
  class NoteFile
    def pread(length, offset) = "hello".byteslice(offset, length)
    def size = 5
  end

  # A NoteFile whose fsync refuses.
  class RefusingFile < NoteFile
    def fsync = raise(Errno::ENOTSUP)
  end

  # The sync's answer, or the class it raised.
  def synced(file_class, path)
    script = "begin; File.open(#{path.dump}) { |file| file.fsync }; rescue SystemCallError => e; e.class.to_s; end"
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", NoteFilesystem.new(file_class)).eval(script).value
  end

  # ENOTSUP is also what a file lacking +fsync+ answers inside the host, so
  # only the two answering apart tell a refusal from a file with none.
  # @behavior RF-021
  def test_a_sync_a_ruby_file_refuses_fails_while_one_it_lacks_succeeds
    assert_equal ["Errno::ENOTSUP", 0], [RefusingFile, NoteFile].map { |file_class| synced(file_class, "/data/note") },
                 "File#fsync of a Ruby file whose fsync raises Errno::ENOTSUP must raise it, and of one " \
                 "lacking fsync must answer 0"
  end

  # @behavior RF-036
  def test_a_ruby_directory_syncs
    assert_equal 0, synced(NoteFile, "/data"),
                 "File#fsync of a Ruby Filesystem's directory must answer 0, as FUSE's answers"
  end
end
