# frozen_string_literal: true

require "test_helper"

# E2E — a script appending to a file a Ruby Filesystem opened, through the
# real Guest Binary's File.
class TestE2ERubyFilesystemAppend < Minitest::Test
  include E2eGuestHelper

  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # One note held in a String, its file opened as +file_class+.
  class NoteFilesystem
    attr_reader :note

    def initialize(file_class)
      @note = +"hello"
      @file_class = file_class
    end

    def open(_path, _flags) = @file_class.new(@note)
    def stat(path) = Stat.new(directory: path.empty?, size: @note.bytesize, ino: path.empty? ? 1 : 2)
  end

  # A file answering the write subset without an append.
  class NoteFile
    def initialize(note) = @note = note

    def pread(length, offset) = @note.byteslice(offset, length)
    def size = @note.bytesize

    def pwrite(data, offset)
      @note[offset, data.bytesize] = data
      data.bytesize
    end
  end

  # A NoteFile appending in one step, answering where the file then ends,
  # and keeping each append it took.
  class AppendingFile < NoteFile
    class << self
      attr_accessor :appends
    end

    def append(data)
      self.class.appends << data
      @note << data
      @note.bytesize
    end
  end

  APPEND = 'File.open("/data/note.txt", "a") { |f| f.write(" world") }'

  def run_script(filesystem, script)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem, access: :read_write).eval(script).value
  end

  # The bytes land where a length read and a pwrite would put them too, so
  # only the append the file kept shows which way they went.
  # @behavior RF-018
  def test_a_script_appends_through_a_ruby_filesystems_file
    AppendingFile.appends = []
    filesystem = NoteFilesystem.new(AppendingFile)
    run_script(filesystem, APPEND)

    assert_equal ["hello world", [" world"]], [filesystem.note, AppendingFile.appends],
                 "IO#write to a file opened to append through a Ruby Filesystem must land at its end " \
                 "through the file's append"
  end

  # @behavior RF-019
  def test_a_ruby_filesystems_file_without_append_refuses_an_append
    filesystem = NoteFilesystem.new(NoteFile)
    answer = run_script(filesystem, "begin; #{APPEND}; rescue Errno::ENOTSUP; \"refused\"; end")

    assert_equal %w[refused hello], [answer, filesystem.note],
                 "IO#write to a file opened to append through a Ruby Filesystem whose file answers no append " \
                 "must raise Errno::ENOTSUP and leave the file as it was"
  end
end
