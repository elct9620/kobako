# frozen_string_literal: true

require "test_helper"
require "socket"

# E2E — a script's UDPSocket through the real Guest Binary against a loopback
# peer: a datagram both ways, and what a socket with no named peer raises.
class TestE2ENetworkUdp < Minitest::Test
  include E2eGuestHelper
  include UdpLoopback

  PING = <<~RUBY
    socket = UDPSocket.new
    socket.bind("0.0.0.0", 0)
    socket.connect("127.0.0.1", %<port>d)
    socket.connect("127.0.0.1", %<port>d)
    socket.send("ping", 0)
    answer = socket.recvfrom(16)
    socket.close
    answer
  RUBY

  REFUSALS = <<~RUBY
    refused = lambda do |&call|
      call.call
      :passed
    rescue SystemCallError => e
      e.class.to_s
    end
    socket = UDPSocket.new
    [
      refused.call { socket.bind("0.0.0.0", 4000) },
      refused.call { socket.send("ping", 0) },
      refused.call { socket.send("ping", 0, "127.0.0.1", %<port>d) },
      refused.call { socket.bind("0.0.0.0", 0) }
    ]
  RUBY

  UDP_IO = <<~RUBY
    socket = UDPSocket.new
    shape = [IPSocket, BasicSocket, IO].map { |ancestor| socket.is_a?(ancestor) } << socket.fileno.is_a?(Integer)
    selected = IO.select(nil, [socket], nil, 0)&.dig(1) == [socket]
    refused = begin
      socket.write("x")
      :written
    rescue Errno::EDESTADDRREQ => e
      e.message
    end
    socket.connect("127.0.0.1", %<port>d)
    written = socket.write("ab", "cd")
    socket.close
    [shape, selected, refused, written]
  RUBY

  SHUT_READING = <<~RUBY
    socket = UDPSocket.new
    socket.connect("127.0.0.1", %<port>d)
    socket.shutdown(Socket::SHUT_RD)
    selected = IO.select([socket], nil, nil, 0)&.first == [socket]
    waited = socket.wait_readable(0) == socket
    pending = socket.recv_nonblock(16, exception: false)
    socket.send("ping", 0)
    [selected, waited, pending, socket.recv(16)]
  RUBY

  # @behavior NW-158
  def test_a_udp_socket_shut_for_reading_polls_readable_yet_still_receives
    with_upcase_peer do |port|
      value = udp_sandbox(port).eval(format(SHUT_READING, port: port)).value

      assert_equal [true, true, :wait_readable, "PING"], value,
                   "after UDPSocket#shutdown(SHUT_RD), IO.select and #wait_readable must find the socket readable " \
                   "at once, as Linux's datagram_poll answers, while #recv_nonblock still answers :wait_readable " \
                   "and #recv still waits for and reads the peer's answer, as Linux's udp_recvmsg delivers"
    end
  end

  # @behavior NW-079
  def test_a_udp_socket_is_an_io_that_io_select_waits_on
    with_quiet_udp_peer do |port, peer|
      value = udp_sandbox(port).eval(format(UDP_IO, port: port)).value

      assert_equal [[[true, true, true, true], true, "Destination address required", 4], "abcd"],
                   [value, receive_first_datagram(peer)],
                   "UDPSocket must be an IPSocket, a BasicSocket, and an IO with an Integer fileno; IO.select must " \
                   "answer a new one writable; #write before #connect must raise Errno::EDESTADDRREQ as CRuby's " \
                   "IO#write does; and #write of two strings after #connect must reach the peer as one datagram"
    end
  end

  # The first datagram +peer+ receives within a few seconds, or nil when none
  # came: a sent datagram may still be on its way once the run returns, since
  # loopback delivers it apart from the send.
  def receive_first_datagram(peer)
    peer.recv(16) if peer.wait_readable(5)
  end

  # @behavior NW-056
  def test_a_udp_socket_carries_a_datagram_both_ways_with_the_peer_it_connects_to
    with_upcase_peer do |port|
      value = udp_sandbox(port).eval(format(PING, port: port)).value

      assert_equal ["PING", ["AF_INET", port, "127.0.0.1", "127.0.0.1"]], value,
                   "UDPSocket#recvfrom after a send to the peer #connect names, the socket bound to " \
                   "port 0 first and connected twice, must answer the peer's datagram and its address"
    end
  end

  # @behavior NW-057
  def test_a_udp_socket_with_no_named_peer_raises_what_linux_would
    value = udp_sandbox(9).eval(format(REFUSALS, port: 9)).value

    assert_equal ["Errno::EACCES", "Errno::EDESTADDRREQ", "Errno::EACCES", :passed], value,
                 "UDPSocket#bind to a chosen port must raise Errno::EACCES, #send with no peer " \
                 "Errno::EDESTADDRREQ, #send to an address no #connect named Errno::EACCES, and " \
                 "#bind to port 0 must pass"
  end
end
