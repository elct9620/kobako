# frozen_string_literal: true

require "test_helper"

# E2E — a script's socket read by byte and by character, through the real
# Guest Binary against a loopback server: a whole UTF-8 sequence, a byte that
# starts none, what ungetc and ungetbyte push back, and the end.
class TestE2ENetworkSocketChars < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  CHARS = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue EOFError => e
      e.class.to_s
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("\\r\\n")
    first = [socket.getc, socket.getc, socket.readchar, socket.getbyte, socket.readbyte]
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
    [first, pushed, line, bytes, chars, yielded, ended]
  RUBY

  ANSWERS = [
    ["h", "é", "l", 108, 111],
    [nil, nil, nil, nil, "BAXY"],
    "\n",
    [255, 119],
    [[111], [114], [10], [122]],
    true,
    [nil, "EOFError", nil, "EOFError", "EOFError", true]
  ].freeze

  # @behavior NW-134
  def test_a_script_reads_a_socket_by_byte_and_by_character_as_crubys
    with_answering_server("héllo\n\xFFwor\nz".b) do |port|
      value = sandbox_reaching(port).eval(format(CHARS, port: port)).value

      assert_equal ANSWERS, value,
                   "TCPSocket#getc and #readchar must take a whole UTF-8 sequence and #getbyte and #readbyte one " \
                   "byte; #ungetc and #ungetbyte must put back what comes first; #each_byte and #each_char must " \
                   "yield to the end; at the end the get forms must answer nil and the read forms raise EOFError"
    end
  end
end
