# frozen_string_literal: true

require "test_helper"

# E2E — a script syncing a file it opened, and a captured stream. Each literal
# is what CRuby 3.4 on Linux answers for the same calls.
class TestE2EFileSync < Minitest::Test
  include E2eGuestHelper

  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # One note, its file counting each sync it is asked for.
  class NoteFilesystem
    attr_reader :syncs

    def initialize = @syncs = 0

    def open(_path, _flags) = NoteFile.new(self)
    def stat(path) = Stat.new(directory: path.empty?, size: 5, ino: path.empty? ? 1 : 2)
    def synced = @syncs += 1
  end

  # A file answering the read subset and counting its syncs on its Filesystem.
  class NoteFile
    def initialize(filesystem) = @filesystem = filesystem

    def pread(length, offset) = "hello".byteslice(offset, length)
    def size = 5
    def fsync = @filesystem.synced
  end

  SYNCED = 'File.open("/data/note") { |file| [file.fsync, file.fdatasync] }'

  CAPTURED = <<~RUBY
    %i[fsync fdatasync].map { |call| begin; $stdout.public_send(call); rescue SystemCallError => e; e.class.to_s; end }
  RUBY

  # @behavior FL-059
  def test_a_sync_reaches_the_files_filesystem
    filesystem = NoteFilesystem.new
    answer = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem).eval(SYNCED).value

    assert_equal [[0, 0], 2], [answer, filesystem.syncs],
                 "File#fsync and File#fdatasync through the bundled guest must each answer 0 and ask the " \
                 "Ruby Filesystem's file to sync"
  end

  # @behavior FL-060
  def test_a_sync_of_a_captured_stream_raises_einval
    assert_equal %w[Errno::EINVAL Errno::EINVAL], Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(CAPTURED).value,
                 "$stdout.fsync and $stdout.fdatasync through the bundled guest must raise Errno::EINVAL, " \
                 "as CRuby does on a pipe"
  end
end
