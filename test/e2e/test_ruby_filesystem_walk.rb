# frozen_string_literal: true

require "test_helper"

# E2E — how often a guest's path reaches a Filesystem written in Ruby,
# through the real Guest Binary's File.
class TestE2ERubyFilesystemWalk < Minitest::Test
  include E2eGuestHelper

  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # One file three directories deep, recording each path its `stat` is
  # asked about.
  class DeepFilesystem
    attr_reader :asked

    def initialize
      @asked = []
    end

    def open(path, _flags)
      raise Errno::ENOENT, path unless path == "a/b/c/note.txt"

      StringIO.new(+"deep")
    end

    def stat(path)
      @asked << path
      return Stat.new(directory: true, size: 7, ino: 1) if ["", "a", "a/b", "a/b/c"].include?(path)
      raise Errno::ENOENT, path unless path == "a/b/c/note.txt"

      Stat.new(directory: false, size: 4, ino: 2)
    end
  end

  # A walk reads what kind each name is, as Linux's reads the dentry, so the
  # read itself asks the object nothing more.
  # @behavior RF-022
  def test_a_path_through_directories_asks_stat_about_each_name
    filesystem = DeepFilesystem.new
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", filesystem, access: :read)
                   .eval('File.read("/data/a/b/c/note.txt")')

    assert_equal ["a", "a/b", "a/b/c", "a/b/c/note.txt", "a/b/c/note.txt"], filesystem.asked,
                 "a file three directories deep read through Sandbox#mount of a Ruby Filesystem must ask its stat " \
                 "once about each name on the way, and again about the file once open"
  end

  # @behavior RF-026
  def test_a_directory_reports_the_size_its_stat_answers
    size = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                          .mount("/data", DeepFilesystem.new, access: :read)
                          .eval('File.size("/data/a/b")').value

    assert_equal 7, size,
                 "File.size of a directory a Ruby Filesystem holds, mounted by Sandbox#mount, must answer the " \
                 "size its stat answers"
  end

  # A file whose +size+ differs from what its Filesystem's +stat+ answers, so
  # a script tells which of them answered.
  class SizedApartFilesystem
    # One open file, answering a size of its own.
    OpenFile = Data.define(:bytes) do
      def size = bytes.bytesize
      def pread(length, offset) = bytes.byteslice(offset, length) || ""
    end

    def initialize
      @names = { "f" => OpenFile.new("hello") }
    end

    def open(path, _flags) = @names.fetch(path) { raise Errno::ENOENT, path }

    def stat(path)
      return Stat.new(true, 0, 1) if path.empty?
      raise Errno::ENOENT, path unless @names.key?(path)

      Stat.new(false, 105, 2)
    end

    def unlink(path) = @names.delete(path)
  end

  SIZES_APART = <<~RUBY
    file = File.open("/data/f")
    sizes = [file.size, File.size("/data/f")]
    File.unlink("/data/f")
    sizes << file.size
  RUBY

  # Only a walk taking a dentry of its own past the file open on the old one
  # keeps the path asking +stat+, and only the file kept on that old dentry
  # answers once the name is gone.
  # @behavior RF-027
  def test_an_open_file_keeps_its_answers_apart_from_its_paths
    sizes = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                           .mount("/data", SizedApartFilesystem.new, access: :read_write)
                           .eval(SIZES_APART).value

    assert_equal [5, 105, 5], sizes,
                 "File#size and File.size through Sandbox#mount of a Ruby Filesystem must answer the open " \
                 "file's size and the path's stat, and File#size must still answer once the name is removed"
  end

  # A directory that becomes a file under the same inode number once a file in
  # it opens, while still answering for the name beneath it.
  class KindChangingFilesystem
    def open(_path, _flags)
      @replaced = true
      StringIO.new(+"held")
    end

    def stat(path)
      case path
      when "" then Stat.new(true, 0, 1)
      when "dir" then Stat.new(!@replaced, 0, 2)
      when "dir/inner" then Stat.new(false, 4, 3)
      else raise Errno::ENOENT, path
      end
    end
  end

  # The open file keeps the dentry of +dir+ alive past the change, and +stat+
  # still answers beneath it, so only comparing the kind stops the walk there.
  # @behavior RF-035
  def test_a_name_whose_kind_changed_is_found_afresh
    exists = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                            .mount("/data", KindChangingFilesystem.new, access: :read)
                            .eval('held = File.open("/data/dir/inner"); [File.exist?("/data/dir/inner"), held.read]')
                            .value

    assert_equal [false, "held"], exists,
                 "File.exist? through Sandbox#mount of a Ruby Filesystem whose directory's stat now answers a file " \
                 "under the same ino must not walk through it, while a file already open still reads"
  end
end
