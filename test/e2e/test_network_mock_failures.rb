# frozen_string_literal: true

require "test_helper"

# E2E — how a script's connects and requests fail when the network beneath
# them fails, stalls, or cannot say the host's addresses, staged through a
# Kobako::Network::Mock.
class TestE2ENetworkMockFailures < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  PUBLIC = "93.184.216.34"

  CONNECT = <<~RUBY
    begin
      TCPSocket.new("app.test", 80)
    rescue => e
      e.class.to_s
    end
  RUBY

  # Each host errno a dial fails with, and the Errno the guest raises for
  # the wasi:sockets code it answers, read through wasi-libc's mapping:
  # unknown reads as EIO, remote-unreachable as EHOSTUNREACH,
  # out-of-memory as ENOMEM, invalid-state as ENOTCONN, and not-supported
  # as ENOTSUP.
  HOST_FAILURES = {
    "EPIPE" => "Errno::EIO", "EADDRINUSE" => "Errno::EADDRINUSE", "ECONNRESET" => "Errno::ECONNRESET",
    "ENETDOWN" => "Errno::EHOSTUNREACH", "ENOBUFS" => "Errno::ENOMEM", "EDESTADDRREQ" => "Errno::ENOTCONN",
    "ENOPROTOOPT" => "Errno::ENOTSUP", "EINVAL" => "Errno::EINVAL"
  }.freeze

  # What the guest's connect to `app.test` raises when the host fails it
  # with the errno +name+.
  def raised_for(name)
    mock = mock_answering({ "app.test" => PUBLIC })
    mock.refuse(PUBLIC, 80, Errno.const_get(name))
    mock_sandbox(mock) { |net| add_tcp_hosts(net, "app.test") }.eval(CONNECT).value
  end

  # @behavior NW-074
  def test_a_host_failure_answers_the_code_wasmtime_wasi_gives_it
    skip "a host errno no io::ErrorKind names is read on Unix alone" if Gem.win_platform?

    assert_equal HOST_FAILURES, HOST_FAILURES.keys.to_h { |name| [name, raised_for(name)] },
                 "a connect failing on the host with each errno must raise what its wasi:sockets code reads as, " \
                 "the code wasmtime-wasi gives that errno"
  end

  HTTP_EACH = <<~RUBY
    %<hosts>p.map do |host|
      Net::HTTP.get(host, "/")
    rescue => e
      e.class.to_s
    end
  RUBY

  # An entry recording each read of its endpoints, each name its resolve
  # hook is asked, and each host its connect hook judges into +seen+, its
  # connect hook refusing `refused.test`.
  def watched_entry(seen)
    entry = Object.new
    entry.define_singleton_method(:endpoints) do
      seen[:reads] << :read
      %w[refused.test ::1].map { |host| Kobako::Network::Endpoint.new(host: host, port: 80) }
    end
    entry.define_singleton_method(:resolve?) { |name| seen[:resolved] << name }
    entry.define_singleton_method(:connect?) { |connection| (seen[:judged] << connection.host).last != "refused.test" }
    entry
  end

  # @behavior NW-077
  def test_a_request_is_judged_once_in_order_and_never_over_ipv6
    mock = mock_answering({ "refused.test" => PUBLIC })
    seen = { reads: [], resolved: [], judged: [] }
    sandbox = Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM, network: mock)
                             .network(:watched, watched_entry(seen))
    value = sandbox.eval(format(HTTP_EACH, hosts: %w[refused.test [::1]])).value

    expected_seen = { reads: [:read], resolved: [], judged: ["refused.test", "::1"] }

    assert_equal [%w[Errno::EACCES Errno::EACCES], expected_seen, []], [value, seen, mock.lookups],
                 "a request a connect hook refuses must ask no resolve hook and no resolver, an IPv6 authority an " \
                 "endpoint names must meet the connect hook and then be refused, and the endpoints must be read once"
  end

  SLOW_THEN_NEXT = <<~RUBY
    http = Net::HTTP.new(%<first>p, 80)
    http.open_timeout = 0.2
    [begin; http.get("/"); rescue => e; e.class.to_s; end,
     begin; Net::HTTP.get("next.test", "/"); rescue => e; e.class.to_s; end]
  RUBY

  # @behavior NW-085
  def test_a_requests_lookup_holds_its_place_until_the_resolver_returns
    mock = mock_answering({ "slow.test" => PUBLIC, "next.test" => PUBLIC })
    sandbox = mock_sandbox(mock, open_connection_limit: 1) do |net|
      %w[slow.test next.test].each { |host| net.endpoint host, port: 80 }
    end
    held = mock.hold("slow.test") { sandbox.eval(format(SLOW_THEN_NEXT, first: "slow.test")).value }
    released = sandbox.eval(format(SLOW_THEN_NEXT, first: "next.test")).value

    assert_equal [%w[Net::OpenTimeout Errno::EAGAIN], %w[Errno::ECONNREFUSED Errno::ECONNREFUSED]], [held, released],
                 "with open_connection_limit 1, a request sent after one whose name is still resolving was given " \
                 "up must raise Errno::EAGAIN until the resolver returns, then dial"
  end

  # @behavior NW-154
  def test_a_connect_or_a_request_is_refused_when_the_hosts_own_addresses_cannot_be_read
    mock = mock_answering({ "app.test" => PUBLIC, "web.test" => PUBLIC }, local_addresses: nil)
    sandbox = mock_sandbox(mock) do |net|
      add_tcp_hosts(net, "app.test")
      net.endpoint "web.test", port: 80
    end
    answers = [sandbox.eval(CONNECT).value, sandbox.eval(format(HTTP_EACH, hosts: ["web.test"])).value]

    assert_equal ["Errno::EACCES", ["Errno::EACCES"], []], [*answers, mock.dials],
                 "with a network that cannot say the host's own addresses, a connect to a name and an HTTP request " \
                 "to one must each be refused, dialing nothing"
  end
end
