# frozen_string_literal: true

require "test_helper"

# E2E — the UDPSocket surface a script shares with CRuby, through the real
# Guest Binary against a loopback peer: the reads that share what a socket
# holds, and the calls CRuby's refuses.
class TestE2ENetworkUdpCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  READ = <<~RUBY
    socket = UDPSocket.new
    socket.connect("127.0.0.1", %<port>d)
    socket.send("ping", 0)
    [socket.read(2), socket.read(2)]
  RUBY

  # @behavior NW-095
  def test_a_udp_socket_reads_one_whole_datagram_through_what_it_holds
    with_upcase_peer do |port|
      value = sandbox_sending_to(port).eval(format(READ, port: port)).value

      assert_equal %w[PI NG], value,
                   "UDPSocket#read(2) must take the peer's whole datagram, answering its first two bytes and " \
                   "holding back the rest for the next read, as CRuby's IO does"
    end
  end

  REFUSES = <<~RUBY
    refused = lambda do |&call|
      call.call
      :passed
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
    socket = UDPSocket.new
    [
      refused.call { UDPSocket.new(99) }.first,
      refused.call { UDPSocket.new(Socket::AF_INET, 0) }.last,
      refused.call { socket.send("ping") }.last,
      refused.call { socket.recv }.last,
      refused.call { Socket.new(2, 1) }.first
    ]
  RUBY

  # @behavior NW-098
  def test_a_udp_socket_refuses_what_crubys_refuses
    value = sandbox_sending_to(9).eval(REFUSES).value

    assert_equal ["Errno::EAFNOSUPPORT", "wrong number of arguments (given 2, expected 0..1)",
                  "wrong number of arguments (given 1, expected 2..4)",
                  "wrong number of arguments (given 0, expected 1..2)", "NoMethodError"], value,
                 "UDPSocket.new of a family other than AF_INET must raise Errno::EAFNOSUPPORT; UDPSocket.new, " \
                 "#send, and #recv with a wrong argument count must raise ArgumentError naming CRuby's range; " \
                 "and Socket.new must raise NoMethodError"
  end
end
