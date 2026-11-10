# frozen_string_literal: true

require "test_helper"

# Differential parity — a network entry's endpoints: both frontends hand the
# guest the same ruleset, so what it refuses and how the guest sees each
# refusal agree. No case reaches a peer, so none waits on the host.
class TestParityNetwork < Parity::Case
  LOOPBACK = [
    { name: "local", endpoints: [{ host: "127.0.0.1", port: 9, protocol: "tcp", addresses: ["local"] }] }
  ].freeze

  REFUSED_LITERAL = <<~RUBY
    begin
      TCPSocket.new("127.0.0.1", 10)
    rescue SystemCallError => e
      [e.class.to_s, e.message]
    end
  RUBY

  NAME_UNRESOLVABLE = <<~RUBY
    begin
      TCPSocket.new("unnamed.example", 9)
    rescue Socket::ResolutionError => e
      [e.class.to_s, e.message, e.error_code]
    end
  RUBY

  UDP_NO_PEER = <<~RUBY
    socket = UDPSocket.new
    [
      (socket.bind("0.0.0.0", 4000) rescue $!.class.to_s),
      (socket.send("ping", 0) rescue $!.class.to_s)
    ]
  RUBY

  # Both frontends run +source+ in a Sandbox holding the loopback entry.
  def assert_network_parity(name, source)
    assert_parity Parity::Scenario.new(
      name: name, networks: LOOPBACK, invocations: [{ verb: "eval", source: source }]
    )
  end

  # @behavior NW-017
  def test_a_tcp_socket_no_endpoint_admits_is_refused_alike
    assert_network_parity("network-literal-refused", REFUSED_LITERAL)
  end

  # @behavior NW-034
  def test_a_name_no_endpoint_names_is_unresolvable_alike
    assert_network_parity("network-name-unresolvable", NAME_UNRESOLVABLE)
  end

  # @behavior NW-057
  def test_a_udp_socket_with_no_named_peer_is_refused_alike
    assert_network_parity("network-udp-no-peer", UDP_NO_PEER)
  end
end
