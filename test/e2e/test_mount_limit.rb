# frozen_string_literal: true

require "test_helper"

# E2E — the +size:+ a Host App gives a Tmpfs, reaching the guest's File
# through the real Guest Binary as Errno::ENOSPC.
class TestE2EMountLimit < Minitest::Test
  include E2eGuestHelper

  # Write five bytes beneath +/data+, answering +:refused+ when the
  # Filesystem has no room, then what +note.txt+ reads if it exists.
  SCRIPT = <<~RUBY
    begin
      File.open("/data/scratch", "w") { |file| file.write("five!") }
      [:written]
    rescue Errno::ENOSPC
      [:refused, File.exist?("/data/note.txt") ? File.read("/data/note.txt") : nil]
    end
  RUBY

  def write_beneath(filesystem)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem, access: :read_write).eval(SCRIPT).value
  end

  # The seeded file rides as a bare keyword beside +size:+, so both must
  # still reach the Tmpfs.
  # @behavior MT-013
  def test_a_tmpfs_size_given_in_ruby_bounds_what_the_guest_writes
    assert_equal [:refused, "hello"], write_beneath(Kobako::Filesystem::Tmpfs.new("note.txt" => "hello", size: 8)),
                 "a write past a size given through Tmpfs.new must raise Errno::ENOSPC in the guest " \
                 "and leave the seeded files"
  end

  # @behavior MT-015
  def test_a_tmpfs_refuses_a_keyword_it_does_not_know
    error = assert_raises(ArgumentError, "a misspelled keyword through Tmpfs.new must raise ArgumentError") do
      Kobako::Filesystem::Tmpfs.new("note.txt" => "hello", sise: 8)
    end

    assert_equal "unknown keyword: :sise", error.message,
                 "a misspelled keyword through Tmpfs.new must be named in the ArgumentError"
  end

  # A Rust panic on such a list would reach Ruby as a fatal no Host App can
  # rescue, so the refusal must arrive as an error instead.
  # @behavior MT-024
  def test_a_tmpfs_refuses_files_it_cannot_hold
    messages = [{ "/" => "x" }, { "a" => "x", "a/b" => "y" }, { "a/b" => "y", "a" => "x" }].map do |files|
      assert_raises(ArgumentError) { Kobako::Filesystem::Tmpfs.new(files) }.message
    end

    assert_equal ["a Tmpfs seeds no file at its root",
                  "a Tmpfs seeds a as both a file and a directory",
                  "a Tmpfs seeds a as both a file and a directory"], messages,
                 "files naming the root, or a file where a directory must stand, through Tmpfs.new " \
                 "must raise ArgumentError naming the path"
  end

  # The files are empty, so only +nr_inodes:+, not the bytes they hold, can
  # refuse the third.
  # @behavior MT-017
  def test_nr_inodes_given_in_ruby_bound_what_the_guest_creates
    script = '(1..3).map { |n| begin; File.write("/data/f" + n.to_s, ""); :made; rescue Errno::ENOSPC; :refused; end }'
    made = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                          .mount("/data", Kobako::Filesystem::Tmpfs.new(nr_inodes: 3), access: :read_write)
                          .eval(script).value

    assert_equal %i[made made refused], made,
                 "an entry past nr_inodes given through Tmpfs.new must raise Errno::ENOSPC in the guest"
  end
end
