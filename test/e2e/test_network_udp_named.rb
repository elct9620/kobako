# frozen_string_literal: true

require "test_helper"
require "socket"

# E2E — a script's UDPSocket connected to an endpoint's name, through the real
# Guest Binary against a loopback peer: a send naming that peer again reaches
# it, and one naming another address is refused.
class TestE2ENetworkUdpNamed < Minitest::Test
  include E2eGuestHelper

  ADDRESSED = <<~RUBY
    socket = UDPSocket.new
    socket.connect("localhost", %<port>d)
    sent = [socket.send("by name", 0, "localhost", %<port>d)]
    sent << begin
      socket.send("elsewhere", 0, "127.0.0.1", %<port>d + 1)
    rescue SystemCallError => e
      e.class.to_s
    end
    sent
  RUBY

  # @behavior NW-168
  def test_a_connected_udp_socket_sends_to_the_peer_it_names_again
    peers = loopback_peers
    port = peers.first.addr[1]
    value = localhost_udp_sandbox(port).eval(format(ADDRESSED, port: port)).value

    assert_equal [[7, "Errno::EACCES"], "by name"], [value, receive_first_datagram_of(peers)],
                 "UDPSocket#send naming the host and port #connect named must reach that peer, and #send " \
                 "naming another address must raise Errno::EACCES"
  ensure
    peers&.each(&:close)
  end

  # A Sandbox whose one UDP endpoint is localhost at +port+.
  def localhost_udp_sandbox(port)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM).network(:local) do |net|
      net.endpoint "localhost", port: port, protocol: :udp, addresses: [:local]
    end
  end

  # Peers on IPv4 loopback and, where the host has one, IPv6 loopback at the
  # same port, since localhost may lead to either first.
  def loopback_peers
    v4 = UDPSocket.new.tap { |peer| peer.bind("127.0.0.1", 0) }
    v6 = begin
      UDPSocket.new(Socket::AF_INET6).tap { |peer| peer.bind("::1", v4.addr[1]) }
    rescue SystemCallError
      nil
    end
    [v4, v6].compact
  end

  # The first datagram any of +peers+ receives within a few seconds.
  def receive_first_datagram_of(peers)
    readable, = IO.select(peers, nil, nil, 5)
    readable&.first&.recv(16)
  end
end
