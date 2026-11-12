# frozen_string_literal: true

require "test_helper"

# E2E — a script's socket read and written past its buffer, through the real
# Guest Binary against loopback peers: recv, sysread, and syswrite, what the
# buffer holding bytes refuses, the end, and the receive flags WASI carries.
class TestE2ENetworkSocketDirect < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback
  include UdpLoopback

  DIRECT = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue IOError, SystemCallError => e
      [e.class.to_s, e.message]
    end
    socket = TCPSocket.new("127.0.0.1", %<tcp>d)
    written = socket.syswrite("\\r\\n")
    IO.select([socket])
    first = [written, socket.recv(2), socket.sysread(2), socket.getc]
    held = [outcome.call { socket.recv(1) }, outcome.call { socket.sysread(1) }, socket.read]
    ended = [socket.recv(5), outcome.call { socket.sysread(1) }]
    udp = UDPSocket.new
    udp.connect("127.0.0.1", %<udp>d)
    udp.send("x", 0)
    IO.select([udp])
    [first, held, ended, [udp.getc, outcome.call { udp.recv(10) }, outcome.call { udp.recvfrom(10) }]]
  RUBY

  DIRECT_ANSWERS = [
    [2, "ab", "cd", "e"],
    [["IOError", "recv for buffered IO"], ["IOError", "sysread for buffered IO"], "f\nxyz"],
    [nil, ["EOFError", "end of file reached"]],
    ["h", ["IOError", "recv for buffered IO"], ["IOError", "recv for buffered IO"]]
  ].freeze

  # @behavior NW-133
  def test_a_script_reads_past_a_sockets_buffer_as_crubys
    with_answering_server("abcdef\nxyz") do |tcp|
      with_hello_peer do |udp|
        value = tcp_and_udp_sandbox(tcp, udp).eval(format(DIRECT, tcp: tcp, udp: udp)).value

        assert_equal DIRECT_ANSWERS, value,
                     "TCPSocket#recv and #sysread must answer up to their length past the buffer and #syswrite its " \
                     "length; while a #getc holds bytes back #recv and #sysread must raise IOError on a TCPSocket " \
                     "and #recv on a UDPSocket; at the end #recv must answer nil and #sysread raise EOFError"
      end
    end
  end

  FLAGS = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("\\r\\n")
    IO.select([socket])
    read = [socket.recv(2, 0), socket.recv(2, 64)]
    refused = [2, 1].map do |flag|
      socket.recv(1, flag)
    rescue SystemCallError => e
      [e.class.to_s, e.message]
    end
    [read, refused]
  RUBY

  # @behavior NW-135
  def test_a_receive_flag_wasi_cannot_carry_raises
    with_answering_server("abcd") do |port|
      value = loopback_sandbox(port).eval(format(FLAGS, port: port)).value

      refused = ["Errno::ENOTSUP", "Operation not supported - recvfrom(2)"]
      assert_equal [%w[ab cd], [refused, refused]], value,
                   "TCPSocket#recv with flags 0 and MSG_DONTWAIT must read, and with MSG_PEEK or MSG_OOB raise " \
                   "Errno::ENOTSUP, since WASI carries no receive flag"
    end
  end
end
