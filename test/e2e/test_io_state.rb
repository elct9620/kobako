# frozen_string_literal: true

require "test_helper"

# E2E — what a kobako-io stream carries through real mruby: the descriptor
# a copy keeps after its original closes, the access a wrap takes from its
# descriptor, and the bookkeeping a script cannot see. Most need a file of
# the guest's own, so they run over a Mount.
class TestE2EIoState < Minitest::Test
  include E2eGuestHelper

  # A guest that writes through a copy of a file it already closed — the
  # copy still carries the released descriptor.
  STALE_COPY_SCRIPT = <<~RUBY
    file = File.open("/data/note.txt", "w")
    copy = file.dup
    file.close
    copy.write("escape")
  RUBY

  # A guest that wraps a file it opened, first in a mode File.open honors,
  # then in one it refuses.
  WRAP_FILE_SCRIPT = <<~RUBY
    fd = File.open("/data/note.txt", "r+").fileno
    [File::RDWR, "zz"].map do |mode|
      IO.new(fd, mode).class.to_s
    rescue ArgumentError => e
      e.message
    end
  RUBY

  # A guest that wraps a file it opened only to read in a mode that writes.
  WRAP_READ_ONLY_SCRIPT = <<~RUBY
    fd = File.open("/data/note.txt").fileno
    begin
      IO.new(fd, "w")
      "built"
    rescue SystemCallError => e
      e.class.to_s
    end
  RUBY

  # A guest that wraps a file it opened only to write with no mode, then
  # with nil, reading and writing through each.
  WRAP_UNNAMED_SCRIPT = <<~RUBY
    File.open("/data/note.txt", "w") do |file|
      [IO.new(file.fileno), IO.new(file.fileno, nil)].map do |io|
        [io.write("x"), begin; io.read; rescue IOError => e; e.class.to_s; end]
      end
    end
  RUBY

  def sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Memory.new("note.txt" => ""), access: :read_write)
  end

  # @behavior IO-003
  # The fd allowlist is enforced at the write syscall, not only at
  # construction: a copy keeps the descriptor its original released, so a
  # write through it must be refused rather than reaching write(2) on a
  # number the guest no longer holds. Without the use-point check the
  # construction-time allowlist would be a courtesy.
  def test_io_write_rejects_a_descriptor_its_original_closed
    err = assert_raises(Kobako::SandboxError) { sandbox.eval(STALE_COPY_SCRIPT) }

    assert_includes err.message, "kobako IO writes only to fd",
                    "a write through a copy of a closed File through Sandbox#eval must be refused at IO#write, " \
                    "not forwarded to write(2)"
  end

  # @behavior IO-037
  # A stream's descriptor, path, and sync flag are kobako's bookkeeping;
  # CRuby shows none of them, so a script listing them must see nothing.
  def test_a_stream_shows_no_instance_variable
    lists = sandbox.eval(<<~RUBY).value
      $stdout.sync = false
      [$stdout.instance_variables, File.open("/data/note.txt").instance_variables]
    RUBY

    assert_equal [[], []], lists,
                 "instance_variables on $stdout and an opened File through Sandbox#eval must be empty"
  end

  # @behavior IO-040
  # zz names no access mode, so only a mode read as File.open reads it
  # refuses the second IO; a mode left unread would build both.
  def test_an_io_over_a_guest_file_reads_its_mode_as_file_open_does
    assert_equal ["IO", "invalid access mode zz"], sandbox.eval(WRAP_FILE_SCRIPT).value,
                 "IO.new over a file the guest opened through Sandbox#eval must build with File::RDWR " \
                 "and raise ArgumentError for zz"
  end

  # @behavior IO-041
  # The file was opened only to read, so a writing mode asks an access the
  # descriptor lacks; a mode left unchecked would build the IO.
  def test_an_io_asking_an_access_its_file_lacks_raises_einval
    assert_equal "Errno::EINVAL", sandbox.eval(WRAP_READ_ONLY_SCRIPT).value,
                 'IO.new with "w" over a file the guest opened only to read through Sandbox#eval must raise ' \
                 "Errno::EINVAL"
  end

  # @behavior IO-043
  # CRuby takes the access a descriptor holds when no mode is given, and
  # stdout holds writing alone; a mode left required would refuse both.
  def test_an_io_given_no_mode_writes_to_a_captured_descriptor
    outcome = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval('IO.new(1).write("a"); IO.new(1, nil).write("b")')

    assert_equal "ab", outcome.stdout,
                 "IO.new(1) and IO.new(1, nil) through Sandbox#eval must write to the stdout capture"
  end

  # @behavior IO-042
  # The file holds writing alone, so an IO that takes its descriptor's
  # access writes and refuses to read; one defaulting to "r" would do the
  # reverse, and a mode left required would build neither.
  def test_an_io_given_no_mode_takes_the_access_its_file_holds
    assert_equal [[1, "IOError"], [1, "IOError"]], sandbox.eval(WRAP_UNNAMED_SCRIPT).value,
                 "IO.new with no mode and with nil over a file the guest opened only to write through " \
                 "Sandbox#eval must write and raise IOError on read"
  end
end
