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
