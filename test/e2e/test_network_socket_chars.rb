# frozen_string_literal: true

require "test_helper"

# E2E — a script's socket read by byte and by character, through the real
# Guest Binary against a loopback server: a character is one byte, as a
# binary-mode socket reads, what ungetc and ungetbyte push back, and the end.
class TestE2ENetworkSocketChars < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  CHARS = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue EOFError, RangeError => e
      e.class.to_s
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("\\r\\n")
    first = [socket.getc, socket.getc, socket.readchar, socket.getbyte, socket.readbyte]
    first = first.map { |char| char.is_a?(String) ? char.bytes : char }
    pushed = [socket.ungetc("XY"), socket.ungetc(65), socket.ungetbyte(0x142), socket.ungetbyte(nil), socket.read(4)]
    line = socket.readline
    bytes = []
    socket.each_byte { |byte| bytes << byte; break if bytes.size == 2 }
    chars = []
    yielded = socket.each_char { |char| chars << char.bytes }.equal?(socket)
    ended = [
      socket.getc, outcome.call { socket.readchar }, socket.getbyte, outcome.call { socket.readbyte },
      outcome.call { socket.readline }, socket.each_byte { |byte| bytes << byte }.equal?(socket)
    ]
    byte = [socket.ungetc(233), socket.getbyte, outcome.call { socket.ungetc(300) }]
    [first, pushed, line, bytes, chars, yielded, ended, byte]
  RUBY

  ANSWERS = [
    [[104], [195], [169], 108, 108],
    [nil, nil, nil, nil, "BAXY"],
    "o\n",
    [255, 119],
    [[111], [114], [10], [122]],
    true,
    [nil, "EOFError", nil, "EOFError", "EOFError", true],
    [nil, 233, "RangeError"]
  ].freeze

  # @behavior NW-134
  def test_a_script_reads_a_socket_by_byte_and_by_character_as_crubys
    with_answering_server("héllo\n\xFFwor\nz".b) do |port|
      value = loopback_sandbox(port).eval(format(CHARS, port: port)).value

      assert_equal ANSWERS, value,
                   "TCPSocket#getc, #readchar, #getbyte, and #readbyte must each take one byte, as a binary-mode " \
                   "socket reads; #ungetc and #ungetbyte must put back what comes first, #ungetc refusing a number " \
                   "past 255 with RangeError; #each_byte and #each_char must yield to the end; at the end the get " \
                   "forms must answer nil and the read forms raise EOFError"
    end
  end
end
