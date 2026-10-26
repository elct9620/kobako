# frozen_string_literal: true

require "test_helper"

# E2E — a guest syncing a file a Ruby Filesystem opened. The real Guest
# Binary imports no fd_sync, so a fixture guest syncs the file and answers
# the errno.
class TestE2ERubyFilesystemSync < Minitest::Test
  include GuestGuard

  FIXTURE = TestPaths.fixture("minimal_fsync.wat")

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

  def setup
    require_fixture!(FIXTURE)
  end

  # ENOTSUP is also what a file lacking +fsync+ answers inside the host, so
  # only the two answering apart tell a refusal from a file with none.
  # @behavior RF-021
  def test_a_sync_a_ruby_file_refuses_fails_while_one_it_lacks_succeeds
    synced = [RefusingFile, NoteFile].map do |file_class|
      Kobako::Sandbox.new(wasm_path: FIXTURE).mount("/data", NoteFilesystem.new(file_class)).eval("").value
    end

    assert_equal [58, 0], synced,
                 "fd_sync of a Ruby file whose fsync raises Errno::ENOTSUP must answer ENOTSUP, and of " \
                 "one lacking fsync must succeed"
  end
end
