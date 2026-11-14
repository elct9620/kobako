# frozen_string_literal: true

require "test_helper"

# E2E — reading a file's lines through +File.readlines+ and +File.foreach+,
# the class-level readers that open the file by name. Each literal is what
# CRuby 3.4 on Linux answers for the same file.
class TestE2EFileLinesByName < Minitest::Test
  include E2eGuestHelper

  # The class each call raises, or what it answers.
  ATTEMPT = "attempt = ->(call) { begin; call.call; rescue Exception => e; e.class.to_s; end }\n"

  READ = <<~RUBY
    [File.readlines("/m/note"), File.readlines("/m/note", chomp: true), File.readlines("/m/note", "b"),
     File.readlines("/m/note", ""), File.readlines("/m/note", 2), File.readlines("/m/note", 2.9)]
  RUBY

  WALKED = <<~RUBY
    lines = []
    chomped = []
    answer = File.foreach("/m/note") { |line| lines << line }
    File.foreach("/m/note", chomp: true) { |line| chomped << line }
    [answer, lines, chomped, attempt.(-> { File.foreach("/m/note") })]
  RUBY

  REFUSED = <<~RUBY
    [-> { File.readlines("/m/none") }, -> { File.readlines("/m/d") },
     -> { File.foreach("/m/none") {} }, -> { File.foreach("/m/d") {} }].map(&attempt)
  RUBY

  def sandbox
    filesystem = Kobako::Filesystem::Tmpfs.new("note" => "a\nbb\n\nccc\n", "d/x" => "1")
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/m", filesystem)
  end

  # @behavior FL-089
  def test_a_script_reads_every_line_of_a_file_by_its_name
    assert_equal [["a\n", "bb\n", "\n", "ccc\n"], ["a", "bb", "", "ccc"], ["a\nb", "b", "\n\nccc\n"],
                  ["a\nbb\n\n", "ccc\n"], ["a\n", "bb", "\n", "\n", "cc", "c\n"],
                  ["a\n", "bb", "\n", "\n", "cc", "c\n"]],
                 sandbox.eval(READ).value,
                 "File.readlines of a mounted file through the bundled guest must answer CRuby's lines, " \
                 "alone and with chomp:, a separator, an empty separator, and a limit"
  end

  # @behavior FL-090
  def test_a_script_walks_a_files_lines_by_its_name
    assert_equal [nil, %W[a\n bb\n \n ccc\n], ["a", "bb", "", "ccc"], "ArgumentError"],
                 sandbox.eval(ATTEMPT + WALKED).value,
                 "File.foreach of a mounted file through the bundled guest must yield each line and " \
                 "answer nil, and raise ArgumentError without a block"
  end

  # @behavior FL-091
  def test_a_line_read_by_name_of_a_path_holding_no_file_raises_cruby_errno
    assert_equal %w[Errno::ENOENT Errno::EISDIR Errno::ENOENT Errno::EISDIR],
                 sandbox.eval(ATTEMPT + REFUSED).value,
                 "File.readlines and File.foreach through the bundled guest must raise ENOENT for a " \
                 "missing path and EISDIR for a directory"
  end
end
