# frozen_string_literal: true

require "test_helper"
require "socket"

# E2E — a script's UDPSocket through the real Guest Binary against a loopback
# peer: a datagram both ways, and what a socket with no named peer raises.
class TestE2ENetworkUdp < Minitest::Test
  include E2eGuestHelper

  PING = <<~RUBY
    socket = UDPSocket.new
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

  AS_IO = <<~RUBY
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

  # @behavior NW-079
  def test_a_udp_socket_is_an_io_that_io_select_waits_on
    with_quiet_peer do |peer, port|
      value = sandbox_reaching(port).eval(format(AS_IO, port: port)).value

      assert_equal [[[true, true, true, true], true, "Destination address required", 4], "abcd"],
                   [value, peer.recv_nonblock(16, exception: false)],
                   "UDPSocket must be an IPSocket, a BasicSocket, and an IO with an Integer fileno; IO.select must " \
                   "answer a new one writable; #write before #connect must raise Errno::EDESTADDRREQ as CRuby's " \
                   "IO#write does; and #write of two strings after #connect must reach the peer as one datagram"
    end
  end

  # Yield a loopback peer that answers nothing, and its port.
  def with_quiet_peer
    peer = UDPSocket.new
    peer.bind("127.0.0.1", 0)
    yield peer, peer.addr[1]
  ensure
    peer&.close
  end

  # Yield the port of a loopback peer that answers each datagram upcased.
  def with_upcase_peer
    peer = UDPSocket.new
    peer.bind("127.0.0.1", 0)
    thread = Thread.new do
      mesg, from = peer.recvfrom(16)
      peer.send(mesg.upcase, 0, from[3], from[1])
    end
    yield peer.addr[1]
  ensure
    thread&.kill
    peer&.close
  end

  def sandbox_reaching(port)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM).network(:local) do |net|
      net.endpoint "127.0.0.1", port: port, protocol: :udp, addresses: [:local]
    end
  end

  # @behavior NW-056
  def test_a_udp_socket_carries_a_datagram_both_ways_with_the_peer_it_connects_to
    with_upcase_peer do |port|
      value = sandbox_reaching(port).eval(format(PING, port: port)).value

      assert_equal ["PING", ["AF_INET", port, "127.0.0.1", "127.0.0.1"]], value,
                   "UDPSocket#recvfrom after a send to the peer #connect names must answer the " \
                   "peer's datagram and its address"
    end
  end

  # @behavior NW-057
  def test_a_udp_socket_with_no_named_peer_raises_what_linux_would
    value = sandbox_reaching(9).eval(format(REFUSALS, port: 9)).value

    assert_equal ["Errno::EACCES", "Errno::EDESTADDRREQ", "Errno::EACCES", :passed], value,
                 "UDPSocket#bind to a chosen port must raise Errno::EACCES, #send with no peer " \
                 "Errno::EDESTADDRREQ, #send to an address no #connect named Errno::EACCES, and " \
                 "#bind to port 0 must pass"
  end
end
