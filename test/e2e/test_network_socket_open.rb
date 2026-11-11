# frozen_string_literal: true

require "test_helper"

# E2E — what a script's socket inherits from CRuby's IO and IPSocket: opening
# with and without a block, and receiving with its sender, through the real
# Guest Binary against a loopback server.
class TestE2ENetworkSocketOpen < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  OPENS = <<~RUBY
    tcp = nil
    udp = nil
    answers = [
      TCPSocket.open("127.0.0.1", %<port>d) { |socket| tcp = socket; :tcp },
      UDPSocket.open { |socket| udp = socket; :udp }
    ]
    answers + [tcp.closed?, udp.closed?, UDPSocket.open.class.to_s]
  RUBY

  # @behavior NW-187
  def test_a_socket_opened_with_a_block_closes_after_it
    with_upcase_server do |port|
      value = loopback_sandbox(port).eval(format(OPENS, port: port)).value

      assert_equal [:tcp, :udp, true, true, "UDPSocket"], value,
                   "TCPSocket.open and UDPSocket.open with a block must answer the block's value and close the " \
                   "socket after it, and UDPSocket.open without one must answer the socket, as CRuby's IO.open does"
    end
  end

  RECVFROM = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("\\r\\n")
    buffer = +""
    [socket.recvfrom(5), socket.recvfrom(5, 0, buffer), buffer]
  RUBY

  # @behavior NW-188
  def test_a_tcp_socket_receives_from_no_named_sender
    with_answering_server("hello world") do |port|
      value = loopback_sandbox(port).eval(format(RECVFROM, port: port)).value

      assert_equal [["hello", nil], [" worl", nil], " worl"], value,
                   "TCPSocket#recvfrom must answer what comes next and nil for the sender, filling outbuf, as " \
                   "CRuby's IPSocket#recvfrom answers a stream"
    end
  end
end
