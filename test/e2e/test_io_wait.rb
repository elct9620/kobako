# frozen_string_literal: true

require "test_helper"

# E2E — a script waiting on a stream that is no socket, through real mruby:
# a file is always ready for the way it was opened, and a wait it was not
# opened for raises, as CRuby's does.
class TestE2EIoWait < Minitest::Test
  include E2eGuestHelper

  WAITS = <<~RUBY
    outcome = lambda do |io, wait|
      io.public_send(wait, 0).equal?(io)
    rescue IOError => e
      e.message
    end
    reader = File.open("/data/note.txt")
    writer = File.open("/data/out.txt", "w")
    closed = File.open("/data/note.txt").tap(&:close)
    [reader, writer, $stdout, closed].map do |io|
      %i[wait_readable wait_writable].map { |wait| outcome.call(io, wait) }
    end
  RUBY

  # @behavior IO-044
  def test_a_script_waits_on_a_file_and_a_captured_stream_as_crubys
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/data", Kobako::Filesystem::Tmpfs.new("note.txt" => "x"), access: :read_write)

    assert_equal [[true, "not opened for writing"], ["not opened for reading", true],
                  ["not opened for reading", true], ["closed stream", "closed stream"]], sandbox.eval(WAITS).value,
                 "IO#wait_readable and #wait_writable through Sandbox#eval must answer a file opened for the wait " \
                 "at once, raise IOError for a wait it was not opened for and on $stdout to read, and raise " \
                 "IOError on a closed file"
  end

  REFUSED_TIMEOUTS = <<~RUBY
    refusal = lambda do |&call|
      call.call
      :waited
    rescue ArgumentError, RangeError => e
      [e.class.to_s, e.message]
    end
    [Float::NAN, -1, Float::INFINITY, 1.0e19, (2**63) - 1].flat_map do |timeout|
      [refusal.call { $stdout.wait_writable(timeout) }, refusal.call { IO.select(nil, [$stdout], nil, timeout) }]
    end
  RUBY

  # @behavior IO-047
  def test_a_timeout_crubys_refuses_raises_as_crubys_does
    expected = [["RangeError", "NaN out of Time range"], ["ArgumentError", "time interval must not be negative"],
                ["RangeError", "Inf out of Time range"],
                ["RangeError", "10000000000000000000.000000 out of Time range"], :waited]
               .flat_map { |refusal| [refusal] * 2 }

    assert_equal expected, Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(REFUSED_TIMEOUTS).value,
                 "IO#wait_writable and IO.select through Sandbox#eval must raise RangeError for a NaN, infinite, " \
                 "or past-time_t timeout and ArgumentError for a negative one, with CRuby's messages, and wait " \
                 "for the Integer 2**63 - 1"
  end
end
