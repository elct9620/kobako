# frozen_string_literal: true

require "test_helper"

# E2E — a script's TCPSocket to a name through the real Guest Binary: the
# virtual address a lookup hands out, and the names a lookup refuses.
class TestE2ENetworkNames < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback
  include UdpLoopback

  NAMED_CONNECT = <<~RUBY
    socket = TCPSocket.new("localhost", %<port>d)
    socket.write("ping")
    socket.close_write
    answer = [socket.read, socket.peeraddr[3]]
    socket.close
    answer
  RUBY

  UNRESOLVED_NAME = <<~RUBY
    begin
      TCPSocket.new(%<name>p, 4567)
    rescue SocketError => e
      [e.class.to_s, e.message, e.error_code]
    end
  RUBY

  # @behavior NW-033
  def test_a_tcp_socket_reaches_a_name_through_its_virtual_address
    with_upcase_server do |port|
      value = localhost_sandbox(port).eval(format(NAMED_CONNECT, port: port)).value

      assert_equal ["PING", localhost_virtual_address("127.0.0.1")], value,
                   "TCPSocket to a name an endpoint names must reach it and show the virtual address as its peer"
    end
  end

  TRIED_IN_TURN = <<~RUBY
    socket = TCPSocket.new("localhost", %<port>d)
    socket.write("ping")
    socket.close_write
    [socket.read, socket.peeraddr[3]]
  RUBY

  # @behavior NW-032
  def test_a_tcp_socket_tries_each_address_a_name_leads_to_in_turn
    with_upcase_server do |port|
      sandbox = localhost_ipv4_only_sandbox(port, protocol: :tcp)
      value = sandbox.eval(format(TRIED_IN_TURN, port: port)).value

      assert_equal ["PING", localhost_virtual_address("127.0.0.1")], value,
                   "TCPSocket to a name whose first addresses an endpoint refuses must try each in turn and " \
                   "connect through the virtual address standing for the one it leads to"
    end
  end

  DATAGRAM_TRIED_IN_TURN = <<~RUBY
    socket = UDPSocket.new
    socket.connect("localhost", %<port>d)
    socket.send("ping", 0, "localhost", %<port>d)
    [socket.recv(16), socket.peeraddr[3]]
  RUBY

  # @behavior NW-195
  def test_a_udp_socket_tries_each_address_a_name_leads_to_in_turn
    with_hello_peer do |port|
      sandbox = localhost_ipv4_only_sandbox(port, protocol: :udp)
      value = sandbox.eval(format(DATAGRAM_TRIED_IN_TURN, port: port)).value

      assert_equal ["hello", localhost_virtual_address("127.0.0.1")], value,
                   "UDPSocket#connect to a name whose first addresses an endpoint refuses must stream with the one " \
                   "it leads to, and a send naming the name must reach that peer, as CRuby's try each address"
    end
  end

  # @behavior NW-034
  def test_a_tcp_socket_to_a_name_no_endpoint_names_raises_resolution_error
    value = loopback_sandbox(4567).eval(format(UNRESOLVED_NAME, name: "elsewhere.example")).value

    assert_equal ["Socket::ResolutionError", "getaddrinfo: Name or service not known", -2], value,
                 "TCPSocket to a name no endpoint names must raise Socket::ResolutionError as NXDOMAIN does"
  end

  # @behavior NW-036
  def test_a_resolve_policy_block_refuses_a_name_an_endpoint_names
    sandbox = localhost_sandbox(4567) { |name| name != "localhost" }
    value = sandbox.eval(format(UNRESOLVED_NAME, name: "LocalHost.")).value

    assert_equal "Socket::ResolutionError", value&.first,
                 "TCPSocket to a name a net.policy(:resolve) block refuses, in any case or with a trailing dot, " \
                 "must raise Socket::ResolutionError"
  end
end
