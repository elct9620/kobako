# frozen_string_literal: true

require "test_helper"

# E2E — the +limit:+ a Host App gives a Memory, reaching the guest's File
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

  # The seeded file rides as a bare keyword beside +limit:+, so both must
  # still reach the Memory.
  # @behavior MT-013
  def test_a_memorys_limit_given_in_ruby_bounds_what_the_guest_writes
    assert_equal [:refused, "hello"], write_beneath(Kobako::Filesystem::Memory.new("note.txt" => "hello", limit: 8)),
                 "a write past a limit given through Memory.new must raise Errno::ENOSPC in the guest " \
                 "and leave the seeded files"
  end

  # @behavior MT-015
  def test_a_memory_refuses_a_keyword_it_does_not_know
    error = assert_raises(ArgumentError, "a misspelled keyword through Memory.new must raise ArgumentError") do
      Kobako::Filesystem::Memory.new("note.txt" => "hello", limt: 8)
    end

    assert_equal "unknown keyword: :limt", error.message,
                 "a misspelled keyword through Memory.new must be named in the ArgumentError"
  end

  # A Rust panic on such a list would reach Ruby as a fatal no Host App can
  # rescue, so the refusal must arrive as an error instead.
  # @behavior MT-024
  def test_a_memory_refuses_files_it_cannot_hold
    messages = [{ "/" => "x" }, { "a" => "x", "a/b" => "y" }, { "a/b" => "y", "a" => "x" }].map do |files|
      assert_raises(ArgumentError) { Kobako::Filesystem::Memory.new(files) }.message
    end

    assert_equal ["a Memory seeds no file at its root",
                  "a Memory seeds a as both a file and a directory",
                  "a Memory seeds a as both a file and a directory"], messages,
                 "files naming the root, or a file where a directory must stand, through Memory.new " \
                 "must raise ArgumentError naming the path"
  end

  # The files are empty, so only +inodes:+, not the bytes they hold, can
  # refuse the third.
  # @behavior MT-017
  def test_inodes_given_in_ruby_bound_what_the_guest_creates
    script = '(1..3).map { |n| begin; File.write("/data/f" + n.to_s, ""); :made; rescue Errno::ENOSPC; :refused; end }'
    made = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                          .mount("/data", Kobako::Filesystem::Memory.new(inodes: 3), access: :read_write)
                          .eval(script).value

    assert_equal %i[made made refused], made,
                 "an entry past inodes given through Memory.new must raise Errno::ENOSPC in the guest"
  end
end
