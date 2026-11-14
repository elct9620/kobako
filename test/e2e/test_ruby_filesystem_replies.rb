# frozen_string_literal: true

require "test_helper"

# E2E — a Ruby Filesystem's file answering more than the guest asked for,
# through the real Guest Binary's File.
class TestE2ERubyFilesystemReplies < Minitest::Test
  include E2eGuestHelper

  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # One note held in a String, its file opened as +file_class+.
  class NoteFilesystem
    def initialize(file_class)
      @note = +"hello"
      @file_class = file_class
    end

    def open(_path, _flags) = @file_class.new(@note)
    def stat(path) = Stat.new(directory: path.empty?, size: @note.bytesize, ino: path.empty? ? 1 : 2)
  end

  # A file that claims ten times what it wrote.
  class OverclaimingFile
    def initialize(note) = @note = note
    def pwrite(data, offset) = (@note[offset, data.bytesize] = data).bytesize * 10
  end

  # A file that answers a read with twice what was asked.
  class OveransweringFile
    def initialize(note) = @note = note
    def pread(length, offset) = @note.byteslice(offset, length) * 2
  end

  def eval_on(file_class, script, access: :read)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", NoteFilesystem.new(file_class), access: access)
                   .eval(script).value
  end

  # @behavior RF-012
  def test_a_write_a_ruby_filesystem_overclaims_raises_eio
    script = 'begin; File.open("/data/note.txt", "w") { |f| f.write("hi") }; rescue Errno::EIO; "refused"; end'

    assert_equal "refused", eval_on(OverclaimingFile, script, access: :read_write),
                 "IO#write whose Ruby file answers more bytes written than it was handed must raise Errno::EIO"
  end

  # @behavior RF-042
  def test_a_read_a_ruby_filesystem_overanswers_raises_eio
    script = 'begin; File.open("/data/note.txt") { |f| f.read(2) }; rescue Errno::EIO; "refused"; end'

    assert_equal "refused", eval_on(OveransweringFile, script),
                 "IO#read whose Ruby file answers pread with more bytes than it was asked for must raise Errno::EIO"
  end
end
