# frozen_string_literal: true

require "test_helper"

# E2E — a value answering to_str where the guest's socket methods take a
# String, through the real Guest Binary, as CRuby's StringValue and
# rb_check_string_type take one.
class TestE2ENetworkToStr < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  TO_STR = <<~RUBY
    class Spelled
      def initialize(text) = @text = text
      def to_str = @text
    end
    socket = TCPSocket.new("127.0.0.1", Spelled.new("%<port>d"))
    socket.ungetc(Spelled.new("x"))
    [
      socket.peeraddr[1],
      Socket.unpack_sockaddr_in(Spelled.new(Socket.sockaddr_in(80, "127.0.0.1"))),
      Socket::Option.new(:INET, :SOCKET, :KEEPALIVE, Spelled.new("\\x01\\x00\\x00\\x00")).int,
      socket.getc,
      Socket.getaddrinfo("127.0.0.1", 80, Spelled.new("INET"), :STREAM).first.first
    ]
  RUBY

  # @behavior NW-207
  def test_a_value_answering_to_str_stands_where_crubys_takes_a_string
    with_listening_server do |port|
      assert_equal [port, [80, "127.0.0.1"], 1, "x", "AF_INET"],
                   loopback_sandbox(port).eval(format(TO_STR, port: port)).value,
                   "a port, a packed address, an option's data, a string pushed back, and a family name each " \
                   "given as an object answering to_str must read as the String it answers, as CRuby's do"
    end
  end
end
