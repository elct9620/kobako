# frozen_string_literal: true

require "test_helper"

# E2E — a script opening a file of a Ruby Filesystem to truncate it, through
# the real Guest Binary's File.
class TestE2ERubyFilesystemTruncate < Minitest::Test
  include E2eGuestHelper

  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # One note, emptied when its open is handed TRUNC. Its file answers no
  # truncate, which the runtime would report as read-only if it asked.
  class NoteFilesystem
    attr_reader :note

    def initialize(note) = @note = +note

    def open(_path, flags)
      @note.clear if flags.anybits?(File::TRUNC)
      NoteFile.new(@note)
    end

    def stat(path) = Stat.new(directory: path.empty?, size: @note.bytesize, ino: path.empty? ? 1 : 2)
  end

  # The note, open.
  class NoteFile
    def initialize(note) = @note = note

    def pread(length, offset) = @note.byteslice(offset, length)

    def pwrite(data, _offset)
      @note << data
      data.bytesize
    end

    def size = @note.bytesize
  end

  # @behavior RF-033
  def test_a_truncating_open_is_answered_by_the_ruby_objects_open
    filesystem = NoteFilesystem.new("older and longer")
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", filesystem, access: :read_write)
                   .eval('File.open("/data/note.txt", "w") { |file| file.write("new") }; nil')

    assert_equal "new", filesystem.note,
                 "File.open with w through a Ruby Filesystem whose file answers no truncate must " \
                 "empty the file through the flags its open was handed"
  end

  # A note whose inode number a replacement changes.
  class ReplacedFilesystem < NoteFilesystem
    def replace = @ino = 3

    def stat(path) = Stat.new(directory: path.empty?, size: note.bytesize, ino: path.empty? ? 1 : @ino || 2)
  end

  # @behavior RF-034
  def test_an_open_reaches_only_the_entry_it_judged
    filesystem = ReplacedFilesystem.new("judged")
    judged = lambda { |_path, _access|
      filesystem.replace
      true
    }
    script = 'begin; File.read("/data/note.txt"); rescue Errno::ENOENT; "gone"; end'
    answer = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem, access: :read, &judged).eval(script)

    assert_equal "gone", answer.value,
                 "File.read through a Ruby Filesystem whose entry a block replaced as it judged the path must raise " \
                 "Errno::ENOENT rather than read what stands there now"
  end
end
