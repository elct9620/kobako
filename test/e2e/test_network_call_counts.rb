# frozen_string_literal: true

require "test_helper"

# E2E — what spends a run's connection_limit: each lookup a resolver is asked
# and each address dialed, the failed ones too, counted before they leave the
# host; and what never leaves the host spends nothing. Staged through a
# Kobako::Network::Mock, so no host socket is made.
class TestE2ENetworkCallCounts < Minitest::Test
  include E2eGuestHelper
  include NetworkSandboxes

  PUBLIC = "93.184.216.34"

  LOOKUPS = <<~RUBY
    (1..%<count>d).map do |i|
      Addrinfo.getaddrinfo("n\#{i}.test", 80)
      :answered
    rescue => e
      e.class.to_s
    end
  RUBY

  # A sandbox under +limit+ whose names under `.test` answer nothing.
  def silent_names_sandbox(mock, limit)
    mock_sandbox(mock, connection_limit: limit) { |net| net.endpoint "*.test", port: 80, protocol: :tcp }
  end

  # @behavior NW-253
  def test_a_lookup_past_the_connection_limit_asks_no_resolver
    mock = Kobako::Network::Mock.new
    silent_names_sandbox(mock, 5).eval(format(LOOKUPS, count: 20))

    assert_equal 5, mock.lookups.size,
                 "20 lookups of names a resolver must be asked, under connection_limit: 5, must ask the resolver " \
                 "5 times, each lookup spending the limit before it leaves the host"
  end

  # @behavior NW-253
  def test_each_run_spends_its_own_connection_limit_on_lookups
    mock = Kobako::Network::Mock.new
    sandbox = silent_names_sandbox(mock, 5)
    2.times { sandbox.eval(format(LOOKUPS, count: 20)) }
    mock_sandbox(mock, connection_limit: 5) { |net| net.endpoint "*.test", port: 80, protocol: :tcp }
      .eval(format(LOOKUPS, count: 20))

    assert_equal 15, mock.lookups.size,
                 "three runs of 20 lookups each under connection_limit: 5, two in one Sandbox and one in a " \
                 "neighbour sharing the Mock, must each ask the resolver 5 times"
  end

  LOOKUPS_THEN_CONNECT = <<~RUBY
    %%w[a.test b.test].each { |name| Addrinfo.getaddrinfo(name, 80) rescue nil }
    begin
      TCPSocket.new(%<address>p, 80)
      :connected
    rescue => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-059
  def test_a_connect_after_lookups_spent_the_limit_answers_eagain_and_dials_nothing
    mock = Kobako::Network::Mock.new
    sandbox = mock_sandbox(mock, connection_limit: 2) do |net|
      net.endpoint "*.test", port: 80, protocol: :tcp
      net.endpoint PUBLIC, port: 80, protocol: :tcp
    end
    value = sandbox.eval(format(LOOKUPS_THEN_CONNECT, address: PUBLIC)).value

    assert_equal ["Errno::EAGAIN", []], [value, mock.dialed_addresses],
                 "a TCPSocket.new after two lookups spent connection_limit: 2 must raise Errno::EAGAIN and dial " \
                 "nothing"
  end

  REQUEST = <<~RUBY
    begin
      Net::HTTP.get("web.test", "/")
      :answered
    rescue => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-254
  def test_each_address_a_request_dials_spends_the_connection_limit
    mock = Kobako::Network::Mock.new
    mock.resolve("web.test", to: (1..10).map { |i| "93.184.216.#{100 + i}" })
    sandbox = mock_sandbox(mock, connection_limit: 1) { |net| net.endpoint "web.test", port: 80 }
    value = sandbox.eval(REQUEST).value

    assert_equal ["Errno::EAGAIN", 1, 1], [value, mock.lookups.size, mock.dialed_addresses.size],
                 "a request to a name of 10 addresses the Mock routes nowhere, under connection_limit: 1, must " \
                 "look the name up once, dial one address, and raise Errno::EAGAIN for connection-limit-reached"
  end
end
