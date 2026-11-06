# frozen_string_literal: true

require "test_helper"

# E2E — a script reading and writing at an offset, and advising how it will
# read. Each literal is what CRuby 3.4 on Linux answers for the same calls.
class TestE2EIOPositioned < Minitest::Test
  include E2eGuestHelper

  # The class each call raises, or what it answers.
  ATTEMPT = "attempt = ->(call) { begin; call.call; rescue Exception => e; e.class.to_s; end }\n"

  POSITIONED = <<~RUBY
    File.open("/m/f", "r+") do |file|
      [file.pread(3, 1), file.pwrite("ZZ", 3), file.pwrite("!", 8), file.pos, File.read("/m/f")]
    end
  RUBY

  AT_THE_END = 'File.open("/m/f") { |file| [attempt.(-> { file.pread(3, 10) }), file.pread(0, 10)] }'

  POSITIONED_REFUSED = <<~RUBY
    File.open("/m/f") do |file|
      [-> { file.pwrite("x", 0) }, -> { $stdout.pread(1, 0) }, -> { $stdout.pwrite("x", 0) },
       -> { file.pread(1, -1) }, -> { file.pread(-1, 0) }].map(&attempt)
    end
  RUBY

  ADVISED = <<~RUBY
    File.open("/m/f") do |file|
      [%i[normal sequential random willneed dontneed noreuse].map { |advice| file.advise(advice, 0, 5) },
       file.read]
    end
  RUBY

  ADVICE_REFUSED = <<~RUBY
    File.open("/m/f") do |file|
      [-> { file.advise("normal") }, -> { file.advise(:bogus) }, -> { file.advise(:normal, 0, -1) },
       -> { $stdout.advise(:normal) }].map(&attempt)
    end
  RUBY

  NEGATIVE_OFFSET = 'File.open("/m/f") { |file| attempt.(-> { file.advise(:normal, -1, 0) }) }'

  CLOSED = <<~RUBY
    file = File.open("/m/f", "r+")
    file.close
    [-> { file.fsync }, -> { file.fdatasync }, -> { file.truncate(0) }, -> { file.pread(1, 0) },
     -> { file.pwrite("x", 0) }, -> { file.advise(:normal) }].map(&attempt)
  RUBY

  def sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/m", Kobako::Filesystem::Tmpfs.new("f" => "hello"),
                                                    access: :read_write)
  end

  # @behavior FL-064
  def test_a_positioned_read_and_write_leave_the_position_alone
    assert_equal ["ell", 2, 1, 0, "helZZ\0\0\0!"], sandbox.eval(POSITIONED).value,
                 "IO#pread and IO#pwrite through the bundled guest must read and write at their offsets, " \
                 "fill a gap with zero bytes, and leave the position at 0"
  end

  # @behavior FL-065
  def test_a_positioned_read_at_the_end_raises_eof_error
    assert_equal ["EOFError", ""], sandbox.eval(ATTEMPT + AT_THE_END).value,
                 "IO#pread past the end through the bundled guest must raise EOFError, and of 0 bytes " \
                 "answer an empty String"
  end

  # @behavior FL-066
  def test_a_positioned_read_or_write_the_stream_cannot_take_raises_what_cruby_raises
    assert_equal %w[IOError IOError Errno::ESPIPE Errno::EINVAL ArgumentError],
                 sandbox.eval(ATTEMPT + POSITIONED_REFUSED).value,
                 "IO#pwrite to a file opened to read, IO#pread and IO#pwrite of $stdout, and IO#pread from " \
                 "a negative offset or length through the bundled guest must raise what CRuby raises"
  end

  # @behavior FL-067
  def test_an_advice_answers_nil_and_changes_nothing_read
    assert_equal [[nil] * 6, "hello"], sandbox.eval(ADVISED).value,
                 "each advice CRuby names through the bundled guest must answer nil and leave the read alone"
  end

  # @behavior FL-068
  def test_an_advice_cruby_refuses_is_refused
    assert_equal %w[TypeError NotImplementedError Errno::EINVAL Errno::ESPIPE],
                 sandbox.eval(ATTEMPT + ADVICE_REFUSED).value,
                 "an advice by a String, an unknown Symbol, or a negative length, and one to $stdout, " \
                 "through the bundled guest must raise what CRuby raises"
  end

  # CRuby on Linux answers nil here; preview1 counts an offset from zero.
  # @behavior FL-069
  def test_an_advice_from_a_negative_offset_raises_einval
    assert_equal "Errno::EINVAL", sandbox.eval(ATTEMPT + NEGATIVE_OFFSET).value,
                 "an advice from offset -1 through the bundled guest must raise Errno::EINVAL"
  end

  # @behavior FL-070
  def test_a_closed_file_refuses_each_call_with_io_error
    assert_equal ["IOError"] * 6, sandbox.eval(ATTEMPT + CLOSED).value,
                 "fsync, fdatasync, truncate, pread, pwrite, and advise on a closed file through the " \
                 "bundled guest must each raise IOError"
  end
end
