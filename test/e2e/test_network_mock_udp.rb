# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox built with network: Kobako::Network::Mock, its guest's
# UDPSocket reaching loopback peers the test runs through the routes the Mock
# keeps for UDP.
class TestE2ENetworkMockUdp < Minitest::Test
  include E2eGuestHelper
  include NetworkSandboxes
  include UdpLoopback

  NAMES = {
    "app.test" => "93.184.216.34", "again.test" => "93.184.216.35",
    "tcp.test" => "93.184.216.36", "refused.test" => "93.184.216.37"
  }.freeze

  EXCHANGE = <<~RUBY
    socket = UDPSocket.new
    socket.connect("app.test", 53)
    socket.send("ping", 0)
    first = socket.recv(16)
    socket.connect("again.test", 53)
    socket.send("ping", 0)
    second = socket.recv(16)
    others = %w[tcp.test refused.test].map do |host|
      UDPSocket.new.connect(host, 53)
      :connected
    rescue SystemCallError => e
      e.class.to_s
    end
    [first, second, *others]
  RUBY

  # A Mock answering NAMES, routing `app.test` over UDP to +upcase+ and
  # `again.test` to +hello+, `tcp.test` over TCP alone, and refusing
  # `refused.test` over UDP.
  def udp_mock(upcase, hello)
    mock = mock_resolving(NAMES)
    mock.route(NAMES["app.test"], 53, to: ["127.0.0.1", upcase], protocol: :udp)
    mock.route(NAMES["again.test"], 53, to: ["127.0.0.1", hello], protocol: :udp)
    mock.route(NAMES["tcp.test"], 53, to: ["127.0.0.1", hello], protocol: :tcp)
    mock.refuse(NAMES["refused.test"], 53, Errno::EADDRNOTAVAIL, protocol: :udp)
    mock
  end

  # What the guest's EXCHANGE answers under +mock+, beside each address the
  # Mock was asked to dial.
  def exchange_through(mock)
    sandbox = mock_sandbox(mock) { |net| NAMES.each_key { |host| net.endpoint host, port: 53, protocol: :udp } }
    [sandbox.eval(EXCHANGE).value, mock.dialed_addresses]
  end

  # @behavior NW-227
  def test_a_mock_routes_a_udp_socket_by_protocol_and_its_peer_answers
    with_upcase_peer do |upcase|
      with_hello_peer do |hello|
        assert_equal [%w[PING hello Errno::ECONNREFUSED Errno::EADDRNOTAVAIL], NAMES.values.map { |ip| [ip, 53] }],
                     exchange_through(udp_mock(upcase, hello)),
                     "a guest UDPSocket under a Mock must read the reply of the peer a UDP route leads to, reach " \
                     "the next peer through the route as it connects again, be refused at connect where only a " \
                     "TCP route or a UDP refusal stands, and the Mock must read back each address dialed"
      end
    end
  end

  def test_a_route_for_a_protocol_a_mock_does_not_dial_raises_argument_error
    assert_raises(ArgumentError, "Mock#route with protocol: :http, which dials as TCP does, must raise " \
                                 "ArgumentError") do
      Kobako::Network::Mock.new.route(NAMES["app.test"], 53, to: ["127.0.0.1", 53], protocol: :http)
    end
  end
end
