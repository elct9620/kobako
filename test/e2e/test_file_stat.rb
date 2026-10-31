# frozen_string_literal: true

require "test_helper"

# E2E — what a script reads through +File::Stat+ and the calls built on one.
# Each literal is what CRuby 3.4 on Linux answers for the same files.
class TestE2EFileStat < Minitest::Test
  include E2eGuestHelper

  A_FILE = <<~RUBY
    stat = File.stat("/d/f")
    [stat.size, stat.size?, stat.zero?, stat.nlink, stat.ftype, stat.file?, stat.directory?,
     stat.symlink?, stat.chardev?, stat.socket?]
  RUBY

  EMPTY_AND_DIRECTORY = <<~RUBY
    empty = File.stat("/d/e")
    dir = File::Stat.new("/d/dir")
    [[empty.size, empty.size?, empty.zero?], [dir.ftype, dir.file?, dir.directory?]]
  RUBY

  CLASS_CALLS = <<~RUBY
    [File.size?("/d/f"), File.size?("/d/e"), File.size?("/d/none"),
     File.zero?("/d/f"), File.zero?("/d/e"), File.zero?("/d/none"), File.empty?("/d/e"),
     File.ftype("/d/f"), File.ftype("/d/dir")]
  RUBY

  OPEN_AND_KEPT = <<~RUBY
    by_path = File.stat("/d/f")
    file = File.open("/d/f")
    kept = file.stat
    File.write("/d/f", "longer!")
    [kept.ino == by_path.ino, file.lstat.ino == by_path.ino, kept.ino != 0, kept.dup.ino == kept.ino,
     kept.size, file.stat.size]
  RUBY

  NAMES_GONE = <<~RUBY
    file = File.open("/d/f")
    replaced = File.open("/d/e")
    moved = File.open("/d/dir/inner")
    File.unlink("/d/f")
    File.rename("/d/dir/inner", "/d/e")
    [file.stat.nlink, file.stat.size, replaced.stat.nlink, moved.stat.nlink]
  RUBY

  def tmpfs_sandbox(profile: :hermetic, access: :read)
    filesystem = Kobako::Filesystem::Tmpfs.new("f" => "hello", "e" => "", "dir/inner" => "x")
    Kobako::Sandbox.new(wasm_path: REAL_WASM, profile: profile).mount("/d", filesystem, access: access)
  end

  # @behavior FL-038
  def test_a_stat_reports_a_file
    assert_equal [5, 5, false, 1, "file", true, false, false, false, false], tmpfs_sandbox.eval(A_FILE).value,
                 "File.stat of a file beneath a Mount must answer its size, kind, and one name"
  end

  # @behavior FL-038
  def test_a_stat_reports_an_empty_file_and_a_directory
    assert_equal [[0, nil, true], ["directory", false, true]], tmpfs_sandbox.eval(EMPTY_AND_DIRECTORY).value,
                 "File.stat of an empty file and File::Stat.new of a directory must answer what each is"
  end

  # @behavior FL-040
  def test_the_class_calls_answer_without_raising_for_a_missing_file
    assert_equal [5, nil, nil, false, true, false, true, "file", "directory"], tmpfs_sandbox.eval(CLASS_CALLS).value,
                 "File.size?, File.zero?, and File.empty? must answer nil or false for a missing file, and " \
                 "File.ftype must name each kind"
  end

  # @behavior FL-043
  def test_a_file_reports_itself_and_a_report_keeps_what_it_read
    answer = tmpfs_sandbox(profile: :permissive, access: :read_write).eval(OPEN_AND_KEPT).value

    assert_equal [true, true, true, true, 5, 7], answer,
                 "File#stat and File#lstat through a permissive guest must read the inode number File.stat does, " \
                 "and a File::Stat must keep the size it read when the file then grows"
  end

  # Linux's count falls to zero with the last name, while the file stays open.
  # @behavior FL-044
  def test_a_file_whose_name_is_gone_counts_no_names
    assert_equal [0, 5, 0, 1], tmpfs_sandbox(access: :read_write).eval(NAMES_GONE).value,
                 "File#stat through a Tmpfs must count no names for a file unlinked or replaced while open, keep " \
                 "its size, and count one for a file moved"
  end
end
