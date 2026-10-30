# frozen_string_literal: true

require "test_helper"

# E2E — a guest looking up through a directory of a Ruby Filesystem it
# holds open after the directory moved. The real Guest Binary holds no
# directory open, so a fixture guest does and answers the lookup's errno.
class TestE2ERubyFilesystemMovedDirectory < Minitest::Test
  include GuestGuard

  FIXTURE = TestPaths.fixture("minimal_moved_directory.wat")

  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # Directories and files held by path, recording each path stat is asked.
  class TreeFilesystem
    attr_reader :asked

    def initialize
      @asked = []
      @entries = { "" => 1, "dir" => 2, "dir/inner" => 3 }
      @directories = ["", "dir"]
    end

    def stat(path)
      @asked << path
      ino = @entries.fetch(path) { raise Errno::ENOENT, path }
      Stat.new(directory: @directories.include?(path), size: 0, ino: ino)
    end

    def open(path, _flags) = raise(Errno::ENOENT, path)

    def mkdir(path)
      @entries[path] = @entries.size + 1
      @directories << path
    end

    def rename(from, to)
      @entries = @entries.to_h { |path, ino| [moved(path, from, to), ino] }
      @directories = @directories.map { |path| moved(path, from, to) }
    end

    private

    def moved(path, from, to) = path == from || path.start_with?("#{from}/") ? to + path.delete_prefix(from) : path
  end

  HELD = TestPaths.fixture("minimal_held_directory.wat")
  ERRNO_NOENT = 44

  # A backend whose `dir` moves beneath `refused/` once its stat is
  # answered, leaving another `dir` holding another `inner`, as a store
  # shared outside the runtime changes behind it.
  class MovingFilesystem
    attr_reader :asked

    def initialize
      @asked = []
      @entries = { "" => 1, "dir" => 2, "dir/inner" => 3 }
      @directories = ["", "dir"]
    end

    def stat(path)
      @asked << path
      ino = @entries.fetch(path) { raise Errno::ENOENT, path }
      Stat.new(directory: @directories.include?(path), size: 0, ino: ino).tap { move_away if path == "dir" }
    end

    def open(path, _flags)
      @asked << path
      raise Errno::ENOENT, path unless @entries.key?(path)

      StringIO.new(+"")
    end

    private

    def move_away
      return if @entries.key?("refused/dir")

      @entries.merge!("refused" => 4, "refused/dir" => 2, "refused/dir/inner" => 3, "dir" => 5, "dir/inner" => 6)
      @directories.push("refused", "refused/dir")
    end
  end

  def setup
    require_fixture!(FIXTURE)
    require_fixture!(HELD)
  end

  # Ruby is asked by the path the descriptor's directory stands at in the
  # runtime, the path the block judges. Another directory stands there now,
  # so the walk ends rather than reach beneath a path the block refuses.
  # @behavior RF-025
  def test_a_directory_a_ruby_filesystems_backend_moved_reaches_nothing_a_policy_refuses
    filesystem = MovingFilesystem.new
    judged = []
    policy = ->(at, _) { (judged << at) && !at.start_with?("/data/refused") }
    errno = Kobako::Sandbox.new(wasm_path: HELD).mount("/data", filesystem, access: :read, &policy).eval("").value

    assert_equal [ERRNO_NOENT, ["/data/dir"], ["dir"]],
                 [errno, judged, filesystem.asked.uniq],
                 "path_open through a directory descriptor a guest opened from a Ruby Filesystem mounted by " \
                 "Sandbox#mount with a block, after the backend moved the directory beneath a refused path, " \
                 "must answer ENOENT and ask the object only about the path the block judged"
  end

  # Another directory made at the old path holds no `inner`, so only a
  # lookup asking by the path the directory moved to finds it.
  # @behavior RF-024
  def test_a_directory_opened_from_a_ruby_filesystem_follows_it_when_it_moves
    filesystem = TreeFilesystem.new
    errno = Kobako::Sandbox.new(wasm_path: FIXTURE).mount("/data", filesystem, access: :read_write).eval("").value

    assert_equal [0, "moved/inner"], [errno, filesystem.asked.last],
                 "path_filestat_get through a directory descriptor a guest opened from a Ruby Filesystem " \
                 "mounted by Sandbox#mount must ask the object by the path the directory moved to"
  end
end
