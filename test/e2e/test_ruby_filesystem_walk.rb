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

  # Each directory on the way would cost one more call into Ruby; the
  # walk instead asks the object once, by the whole path.
  # @behavior RF-022
  def test_a_path_through_directories_asks_stat_about_the_file_alone
    filesystem = DeepFilesystem.new
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", filesystem, access: :read)
                   .eval('File.read("/data/a/b/c/note.txt")')

    assert_equal ["a/b/c/note.txt"], filesystem.asked,
                 "a file three directories deep read through Sandbox#mount of a Ruby Filesystem must ask its stat " \
                 "about the file alone"
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
end
