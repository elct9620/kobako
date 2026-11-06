# frozen_string_literal: true

require "test_helper"

# E2E — a script setting a file's length by its path and through a file it
# opened. Each literal is what CRuby 3.4 on Linux answers for the same calls.
class TestE2EFileTruncate < Minitest::Test
  include E2eGuestHelper

  # The class each call raises, or what it answers.
  ATTEMPT = "attempt = ->(call) { begin; call.call; rescue Exception => e; e.class.to_s; end }\n"

  TRUNCATED = <<~RUBY
    by_path = File.truncate("/m/f", 2)
    by_file = File.open("/m/f", "r+") { |file| file.truncate(4) }
    [by_path, by_file, File.read("/m/f")]
  RUBY

  REFUSED = <<~RUBY
    [-> { File.open("/m/f") { |file| file.truncate(0) } }, -> { File.truncate("/m/f", -1) },
     -> { File.truncate("/m/d", 0) }, -> { File.truncate("/m/none", 0) }].map(&attempt)
  RUBY

  UNMOUNTED = 'attempt.(-> { File.truncate("/m/f", 0) })'

  def sandbox
    filesystem = Kobako::Filesystem::Tmpfs.new("f" => "hello", "d/x" => "1")
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/m", filesystem, access: :read_write)
  end

  # @behavior FL-061
  def test_a_truncate_sets_a_files_length
    assert_equal [0, 0, "he\0\0"], sandbox.eval(TRUNCATED).value,
                 "File.truncate and File#truncate through the bundled guest must each answer 0, shrinking " \
                 "the file and then filling its growth with zero bytes"
  end

  # @behavior FL-062
  def test_a_truncate_a_file_or_a_length_cannot_take_raises_what_cruby_raises
    assert_equal %w[IOError Errno::EINVAL Errno::EISDIR Errno::ENOENT], sandbox.eval(ATTEMPT + REFUSED).value,
                 "a truncate of a file opened to read, by a negative length, of a directory, and of a " \
                 "missing path through the bundled guest must raise what CRuby raises"
  end

  # @behavior FL-063
  def test_a_truncate_by_path_with_nothing_mounted_is_unimplemented
    assert_equal "NotImplementedError", Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(ATTEMPT + UNMOUNTED).value,
                 "File.truncate with nothing mounted through the bundled guest must raise NotImplementedError"
  end
end
