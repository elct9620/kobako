# frozen_string_literal: true

require "test_helper"

# E2E — a script's socket as the IO CRuby's is, through the real Guest Binary
# against a loopback server: the reads that share what a socket holds, the
# calls a socket has no answer for, and a reset stream.
class TestE2ENetworkSocketIo < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  READS = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("\\r\\n")
    buffer = +""
    first = [socket.eof?, socket.gets(chomp: true), socket.read(3, buffer), buffer, socket.gets]
    lines = []
    socket.each_line { |line| lines << line }
    first + [lines, socket.readlines, socket.eof?]
  RUBY

  # @behavior NW-095
  def test_a_tcp_socket_reads_through_what_it_holds_as_crubys_io_does
    with_answering_server("l1\nl2\nl3\nl4\nl5\n") do |port|
      value = sandbox_reaching(port).eval(format(READS, port: port)).value

      assert_equal [false, "l1", "l2\n", "l2\n", "l3\n", %W[l4\n l5\n], [], true], value,
                   "TCPSocket#eof?, #gets(chomp: true), #read(n, buf), #gets, #each_line, and #readlines must " \
                   "answer as CRuby's IO does, each taking first what an earlier call held back"
    end
  end

  NO_POSITION = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      e.class.to_s
    end
    stream = TCPSocket.new("127.0.0.1", %<port>d)
    stream.write("\\r\\n")
    [stream, UDPSocket.new].map do |socket|
      [
        outcome.call { socket.seek(0) }, outcome.call { socket.pos }, outcome.call { socket.rewind },
        outcome.call { socket.fsync }, outcome.call { socket.fdatasync }, outcome.call { socket.advise(:normal) },
        socket.sync, outcome.call { socket.sync = false }, socket.sync,
        outcome.call { socket.dup }, outcome.call { socket.clone }
      ]
    end
  RUBY

  # @behavior NW-096
  def test_a_socket_has_no_position_sync_or_copy
    with_answering_server("") do |port|
      value = sandbox_reaching(port).eval(format(NO_POSITION, port: port)).value

      answers = (["Errno::ESPIPE"] * 3) + (["Errno::EINVAL"] * 2) + [nil, true, false, false] + (["NoMethodError"] * 2)
      assert_equal [answers, answers], value,
                   "TCPSocket and UDPSocket #seek, #pos, and #rewind must raise Errno::ESPIPE, #fsync and " \
                   "#fdatasync Errno::EINVAL, #advise answer nil, #sync start true and read back what #sync= " \
                   "set, and #dup and #clone raise NoMethodError"
    end
  end

  RESET = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("\\r\\n")
    begin
      socket.read
    rescue SystemCallError => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-097
  def test_a_reset_stream_read_raises_econnreset
    with_resetting_server do |port|
      value = sandbox_reaching(port).eval(format(RESET, port: port)).value

      assert_equal "Errno::ECONNRESET", value,
                   "TCPSocket#read from a server that resets the connection must raise Errno::ECONNRESET"
    end
  end

  CONVERTED = <<~RUBY
    wrapper = Class.new do
      def initialize(io) = @io = io
      def to_io = @io
    end.new(TCPSocket.new("127.0.0.1", %<port>d))
    refused = lambda do |&call|
      call.call
    rescue TypeError, SystemCallError => e
      e.class.to_s
    end
    [IO.select(nil, [wrapper], nil, 5)[1] == [wrapper], refused.call { IO.select([1]) },
     refused.call { IO.select(nil, [wrapper], [5], 5) }, refused.call { IO.select([]) }]
  RUBY

  # @behavior NW-110
  def test_io_select_takes_what_converts_to_an_io
    with_listening_server do |port|
      value = sandbox_reaching(port).eval(format(CONVERTED, port: port)).value

      assert_equal [true, "TypeError", "TypeError", "Errno::ENOTSUP"], value,
                   "IO.select must answer an object whose to_io is a TCPSocket ready as it was given, raise " \
                   "TypeError for an Integer in its read or error set, and raise Errno::ENOTSUP with nothing to " \
                   "wait on and no timeout"
    end
  end
end
