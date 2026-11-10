# frozen_string_literal: true

require "test_helper"
require "socket"

# E2E — a script's UDPSocket connected to an endpoint's name, through the real
# Guest Binary against a loopback peer: a send naming that peer again reaches
# it, and one naming another address is refused.
class TestE2ENetworkUdpNamed < Minitest::Test
  include E2eGuestHelper

  ADDRESSED = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      e.class.to_s
    end
    socket = UDPSocket.new
    socket.connect("localhost", %<port>d)
    virtual = socket.remote_address.ip_address
    [
      outcome.call { socket.send("a", 0, "localhost", %<port>d) },
      outcome.call { socket.send("b", 0, "LOCALHOST.", %<port>d) },
      outcome.call { socket.send("c", 0, virtual, %<port>d) },
      outcome.call { socket.send("x", 0, "localhost", %<port>d + 1) },
      outcome.call { socket.send("x", 0, "unnamed.example", %<port>d) },
      outcome.call { socket.send("x", 0, "::1", %<port>d) }
    ]
  RUBY

  # @behavior NW-168
  def test_a_connected_udp_socket_sends_to_the_peer_it_names_again
    peers = loopback_peers
    port = peers.first.addr[1]
    value = localhost_udp_sandbox(port).eval(format(ADDRESSED, port: port)).value

    assert_equal [[1, 1, 1, "Errno::EACCES", "Socket::ResolutionError", "Errno::EAFNOSUPPORT"], %w[a b c]],
                 [value, receive_from_any(peers, 3).sort],
                 "UDPSocket#send naming the host #connect named in any case, or the peer's address, at its port " \
                 "must reach that peer; one to another port must raise Errno::EACCES, and one to a name no " \
                 "endpoint allows or to IPv6 must raise what a connect to it would"
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

  # Up to +count+ datagrams any of +peers+ receives, each within a few seconds.
  def receive_from_any(peers, count)
    Array.new(count).filter_map do
      readable, = IO.select(peers, nil, nil, 5)
      readable&.first&.recv(16)
    end
  end
end
