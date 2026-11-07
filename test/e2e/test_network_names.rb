# frozen_string_literal: true

require "test_helper"

# E2E — a script's TCPSocket to a name through the real Guest Binary: the
# virtual address a lookup hands out, and the names a lookup refuses.
class TestE2ENetworkNames < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  NAMED = <<~RUBY
    socket = TCPSocket.new("localhost", %<port>d)
    socket.write("ping")
    socket.close_write
    answer = [socket.read, socket.peeraddr[3]]
    socket.close
    answer
  RUBY

  UNRESOLVED = <<~RUBY
    begin
      TCPSocket.new(%<name>p, 4567)
    rescue SocketError => e
      [e.class.to_s, e.message, e.error_code]
    end
  RUBY

  def sandbox_naming_localhost(port, &policy)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:local) do |net|
      net.endpoint "localhost", port: port, protocol: :tcp, addresses: [:local]
      net.policy(:resolve, &policy) if policy
    end
  end

  # @behavior NW-033
  def test_a_tcp_socket_reaches_a_name_through_its_virtual_address
    with_upcase_server do |port|
      value = sandbox_naming_localhost(port).eval(format(NAMED, port: port)).value

      assert_equal ["PING", "198.18.0.0"], value,
                   "TCPSocket to a name an endpoint names must reach it and show the virtual address as its peer"
    end
  end

  # @behavior NW-034
  def test_a_tcp_socket_to_a_name_no_endpoint_names_raises_resolution_error
    value = sandbox_reaching(4567).eval(format(UNRESOLVED, name: "elsewhere.example")).value

    assert_equal ["Socket::ResolutionError", "getaddrinfo: Name or service not known", -2], value,
                 "TCPSocket to a name no endpoint names must raise Socket::ResolutionError as NXDOMAIN does"
  end

  # @behavior NW-036
  def test_a_resolve_policy_block_refuses_a_name_an_endpoint_names
    sandbox = sandbox_naming_localhost(4567) { |name| name != "localhost" }
    value = sandbox.eval(format(UNRESOLVED, name: "LocalHost.")).value

    assert_equal "Socket::ResolutionError", value&.first,
                 "TCPSocket to a name a net.policy(:resolve) block refuses, in any case or with a trailing dot, " \
                 "must raise Socket::ResolutionError"
  end
end
