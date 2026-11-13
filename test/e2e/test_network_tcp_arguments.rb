# frozen_string_literal: true

require "test_helper"

# E2E — the arguments a script's TCPSocket reads as CRuby's on Linux does,
# through the real Guest Binary against a loopback server: the port, the
# keywords bounding a connect, and peeraddr's reverse lookup flag.
class TestE2ENetworkTcpArguments < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  PORTS = <<~RUBY
    [
      TCPSocket.new("127.0.0.1", "%<port>d").peeraddr[1],
      TCPSocket.new("127.0.0.1", %<port>d + 65_536).peeraddr[1],
      begin
        TCPSocket.new("127.0.0.1", "http")
      rescue Socket::ResolutionError => e
        [e.error_code, e.message]
      end
    ]
  RUBY

  # @behavior NW-099
  def test_a_port_is_read_as_linux_reads_a_service
    with_listening_server do |port|
      value = loopback_sandbox(port).eval(format(PORTS, port: port)).value

      assert_equal [port, port, [-8, "getaddrinfo: Servname not supported for ai_socktype"]], value,
                   "TCPSocket.new must reach the port a String of digits names and the low 16 bits of an " \
                   "Integer, and a service name must raise Socket::ResolutionError with EAI_SERVICE"
    end
  end

  LOOKUP = <<~RUBY
    timed_out = begin
      TCPSocket.new("localhost", %<port>d, resolv_timeout: 0.000001)
    rescue Errno::ETIMEDOUT => e
      e.message
    end
    [timed_out, TCPSocket.new("localhost", %<port>d, resolv_timeout: 0.000001, fast_fallback: false).peeraddr[1]]
  RUBY

  # @behavior NW-100
  def test_a_tcp_socket_bounds_its_name_lookup_by_resolv_timeout
    with_listening_server do |port|
      value = localhost_sandbox(port).eval(format(LOOKUP, port: port)).value

      assert_equal ["Connection timed out - user specified timeout", port], value,
                   "TCPSocket.new with a resolv_timeout: shorter than the lookup must raise Errno::ETIMEDOUT, " \
                   "and with fast_fallback: false must read no resolv_timeout: and connect"
    end
  end

  FLAGS = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    refused = lambda do |&call|
      call.call
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
    [
      [true, :hostname, :numeric].map { |flag| socket.peeraddr(flag)[2] },
      refused.call { socket.peeraddr(:bogus) },
      refused.call { socket.peeraddr(1) }
    ]
  RUBY

  # @behavior NW-101
  def test_peeraddr_takes_crubys_reverse_lookup_flags
    with_listening_server do |port|
      value = loopback_sandbox(port).eval(format(FLAGS, port: port)).value

      assert_equal [%w[127.0.0.1 127.0.0.1 127.0.0.1],
                    ["ArgumentError", "invalid reverse_lookup flag: :bogus"],
                    ["TypeError", "wrong argument type Integer (expected Symbol)"]], value,
                   "TCPSocket#peeraddr must answer the address for true, :hostname, and :numeric, raise " \
                   "ArgumentError for an unknown Symbol, and TypeError for an Integer"
    end
  end
end
