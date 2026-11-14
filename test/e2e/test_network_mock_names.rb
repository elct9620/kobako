# frozen_string_literal: true

require "test_helper"

# E2E — what a script's names answer and where its connects lead, staged
# through a Kobako::Network::Mock beneath the same rules as the host's own.
class TestE2ENetworkMockNames < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  PUBLIC = "93.184.216.34"

  LOOKUP_EACH = <<~RUBY
    %<names>p.map do |name|
      Addrinfo.getaddrinfo(name, 80)
    rescue SocketError => e
      [e.class.to_s, e.error_code]
    end
  RUBY

  # @behavior NW-024
  def test_a_name_no_endpoint_names_answers_unresolvable_without_asking_the_resolver
    mock = Kobako::Network::Mock.new
    sandbox = mock_sandbox(mock) do |net|
      add_tcp_hosts(net, "app.test", "*.example.test", "denied.test")
      net.policy(:resolve) { |name| name != "denied.test" }
    end
    value = sandbox.eval(format(LOOKUP_EACH, names: %w[nobody.test denied.test example.test])).value

    assert_equal [[["Socket::ResolutionError", -2]] * 3, []], [value, mock.lookups],
                 "a lookup of a name no endpoint names, one a resolve policy refuses, or a wildcard's apex must " \
                 "raise Socket::ResolutionError as NXDOMAIN, and the Mock must be asked nothing"
  end

  CONNECT_EACH = <<~RUBY
    %<hosts>p.map do |host|
      socket = TCPSocket.new(host, 80)
      socket.write("ping")
      socket.close_write
      [socket.read, socket.peeraddr[3]]
    rescue => e
      e.class.to_s
    end
  RUBY

  def connect_each(sandbox, *hosts)
    sandbox.eval(format(CONNECT_EACH, hosts: hosts)).value
  end

  # A Sandbox reaching `public.test` and `local.test` at the public
  # addresses alone, and `cache.test` at local ones.
  def public_and_local_sandbox(mock)
    mock_sandbox(mock) do |net|
      add_tcp_hosts(net, "public.test", "local.test")
      net.endpoint "cache.test", port: 80, protocol: :tcp, addresses: [:local]
    end
  end

  # @behavior NW-025
  def test_a_name_connects_through_its_virtual_address_to_where_its_endpoint_leads
    with_upcase_server do |port|
      mock = mock_answering({ "public.test" => PUBLIC, "local.test" => "127.0.0.1", "cache.test" => "127.0.0.1" })
      mock.route(PUBLIC, 80, to: ["127.0.0.1", port])
      connected = connect_each(public_and_local_sandbox(mock), "public.test", "local.test", "cache.test")

      assert_equal [[%w[PING 198.18.0.0], "Errno::EACCES", "Errno::ECONNREFUSED"], [[PUBLIC, 80], ["127.0.0.1", 80]]],
                   [connected, mock.dials],
                   "a connect to a name must dial the one real address its virtual address stands for, and an " \
                   "endpoint leading only to public addresses must refuse a local one an endpoint naming :local admits"
    end
  end

  PUBLIC_V6 = "2606:2800:220:1:248:1893:25c8:1946"

  # @behavior NW-025
  def test_a_name_whose_real_address_is_ipv6_connects_through_a_virtual_ipv4_one
    with_upcase_server do |port|
      mock = mock_answering({ "v6.test" => PUBLIC_V6 })
      mock.route(PUBLIC_V6, 80, to: ["127.0.0.1", port])
      sandbox = mock_sandbox(mock) { |net| add_tcp_hosts(net, "v6.test") }

      assert_equal [[%w[PING 198.18.0.0]], [[PUBLIC_V6, 80]]], [connect_each(sandbox, "v6.test"), mock.dials],
                   "a connect to a name whose only real address is IPv6 must reach it through a virtual IPv4 " \
                   "address, as the guest has no IPv6, and dial that IPv6 address"
    end
  end

  # @behavior NW-026
  def test_a_virtual_address_no_lookup_handed_out_is_refused
    mock = mock_answering({ "app.test" => PUBLIC })
    sandbox = mock_sandbox(mock) { |net| add_tcp_hosts(net, "app.test") }

    assert_equal [["Errno::EACCES"], []], [connect_each(sandbox, "198.18.0.7"), mock.dials],
                 "TCPSocket to a virtual address no lookup handed out must raise Errno::EACCES and dial nothing"
  end

  # @behavior NW-028
  def test_the_hosts_own_address_is_local
    mock = mock_answering({ "own.test" => PUBLIC }, local_addresses: [PUBLIC])
    sandbox = mock_sandbox(mock) { |net| add_tcp_hosts(net, "own.test") }

    assert_equal [["Errno::EACCES"], []], [connect_each(sandbox, "own.test"), mock.dials],
                 "a name resolving to one of the host's own addresses must be refused by an endpoint leading only " \
                 "to public addresses, whatever registry classes the address, and dial nothing"
  end
end
