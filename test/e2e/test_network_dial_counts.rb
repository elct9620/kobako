# frozen_string_literal: true

require "test_helper"

# E2E — each address a run dials spends its connection_limit before the dial
# leaves the host, a failed one too, while an attempt refused on the host
# spends nothing. Staged through a Kobako::Network::Mock, so no host socket
# is made.
class TestE2ENetworkDialCounts < Minitest::Test
  include E2eGuestHelper
  include NetworkSandboxes

  PUBLIC = "93.184.216.34"

  UDP_CONNECTS = <<~RUBY
    (1..100).map do
      UDPSocket.new.connect(%<address>p, 53)
      :connected
    rescue => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-059
  def test_each_failed_udp_connect_spends_the_limit
    mock = Kobako::Network::Mock.new
    sandbox = mock_sandbox(mock, connection_limit: 5) { |net| net.endpoint PUBLIC, port: 53, protocol: :udp }
    sandbox.eval(format(UDP_CONNECTS, address: PUBLIC))

    assert_equal 5, mock.dialed_addresses.size,
                 "100 UDP connects to an address the Mock routes nowhere, under connection_limit: 5, must dial " \
                 "5 times, each failed dial spending the limit"
  end

  TCP_CONNECTS = <<~RUBY
    %<names>p.map do |name|
      TCPSocket.new(name, 80)
      :connected
    rescue => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-059
  def test_a_connect_to_a_name_spends_its_lookup_and_its_dial
    names = %w[a.test b.test c.test d.test]
    mock = mock_resolving(names.each_with_index.to_h { |name, i| [name, "93.184.216.#{40 + i}"] })
    sandbox = mock_sandbox(mock, connection_limit: 5) { |net| add_tcp_hosts(net, *names) }
    sandbox.eval(format(TCP_CONNECTS, names: names))

    assert_equal [3, 2], [mock.lookups.size, mock.dialed_addresses.size],
                 "four TCPSocket.new to different names, each refused by the Mock, under connection_limit: 5 " \
                 "must look up 3 names and dial 2, each lookup and each dial spending one"
  end

  # @behavior NW-059
  def test_a_failed_tcp_connect_spends_the_limit_once
    mock = Kobako::Network::Mock.new
    sandbox = mock_sandbox(mock, connection_limit: 5) { |net| net.endpoint PUBLIC, port: 80, protocol: :tcp }
    sandbox.eval(format(TCP_CONNECTS, names: [PUBLIC] * 100))

    assert_equal 5, mock.dialed_addresses.size,
                 "100 TCPSocket.new to an address the Mock routes nowhere, under connection_limit: 5, must dial " \
                 "5 times"
  end

  # @behavior NW-059
  def test_a_second_run_starts_from_a_full_limit_and_nil_counts_nothing
    mock = Kobako::Network::Mock.new
    sandbox = mock_sandbox(mock, connection_limit: 5) { |net| net.endpoint PUBLIC, port: 80, protocol: :tcp }
    2.times { sandbox.eval(format(TCP_CONNECTS, names: [PUBLIC] * 10)) }
    mock_sandbox(mock, connection_limit: nil) { |net| net.endpoint PUBLIC, port: 80, protocol: :tcp }
      .eval(format(TCP_CONNECTS, names: [PUBLIC] * 10))

    assert_equal 20, mock.dialed_addresses.size,
                 "two runs of 10 refused connects under connection_limit: 5 must dial 5 each, and a run under " \
                 "connection_limit: nil must dial all 10"
  end

  NEVER_LEAVES = <<~RUBY
    refused = (1..3).map do
      TCPSocket.new(%<refused>p, 80)
    rescue => e
      e.class.to_s
    end
    Addrinfo.getaddrinfo(%<address>p, 80)
    Addrinfo.getaddrinfo("app.test", 80)
    Addrinfo.getaddrinfo("app.test", 80)
    connected = begin
      TCPSocket.new(%<address>p, 80)
      :dialed
    rescue Errno::EAGAIN
      :refused
    rescue SystemCallError
      :dialed
    end
    [refused.uniq, connected]
  RUBY

  REFUSED = "93.184.216.99"

  # A Sandbox under connection_limit: 2 reaching `app.test`, PUBLIC, and
  # REFUSED, whose connect policy refuses REFUSED.
  def guarded_sandbox(mock)
    mock_sandbox(mock, connection_limit: 2) do |net|
      add_tcp_hosts(net, "app.test", PUBLIC, REFUSED)
      net.policy(:connect) { |connection| connection.host != REFUSED }
    end
  end

  # @behavior NW-255
  def test_an_attempt_that_never_leaves_the_host_spends_nothing
    mock = mock_resolving({ "app.test" => PUBLIC })
    value = guarded_sandbox(mock).eval(format(NEVER_LEAVES, refused: REFUSED, address: PUBLIC)).value

    assert_equal [[["Errno::EACCES"], :dialed], ["app.test"]], [value, mock.lookups],
                 "under connection_limit: 2, connects a policy refuses, a lookup of an address, and a second " \
                 "lookup of a translated name must spend nothing, so after one lookup the next connect still dials"
  end
end
