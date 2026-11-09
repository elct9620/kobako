# frozen_string_literal: true

require "test_helper"

# E2E — the socket options a script shares with CRuby beyond keepalive, and
# the ones WASI has no call for, through the real Guest Binary against a
# loopback server.
class TestE2ENetworkSocketOptionsCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  HOP_LIMITS = <<~RUBY
    answer = lambda do |&call|
      call.call
    rescue StandardError => e
      e.class.to_s
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    ttl = [
      socket.getsockopt(:IP, :TTL).int,
      answer.call { socket.setsockopt(:IP, :TTL, 0) },
      answer.call { socket.setsockopt(:IP, :TTL, 256) },
      answer.call { socket.setsockopt(Socket::IPPROTO_IP, Socket::IP_TTL, 1); socket.getsockopt(:IP, :TTL).int },
      answer.call { socket.setsockopt(:IP, :TTL, -1); socket.getsockopt(:IP, :TTL).int }
    ]
    socket.close
    udp = UDPSocket.new
    datagram = [
      udp.getsockopt(:IP, :TTL).int,
      answer.call { udp.setsockopt(:IP, :TTL, 9); udp.getsockopt(:IP, :TTL).int },
      answer.call { udp.setsockopt(:SOCKET, :RCVBUF, 1000); udp.getsockopt(:SOCKET, :RCVBUF).int },
      answer.call { udp.setsockopt(:SOCKET, :SNDBUF, 1000); udp.getsockopt(:SOCKET, :SNDBUF).int },
      answer.call { udp.getsockopt(:TCP, :KEEPIDLE) },
      answer.call { udp.setsockopt(:TCP, :NODELAY, 1) }
    ]
    udp.close
    [ttl, datagram]
  RUBY

  # @behavior NW-126
  def test_a_script_sets_hop_limits_and_udp_options_as_cruby_does
    with_upcase_server do |port|
      value = sandbox_reaching(port).eval(format(HOP_LIMITS, port: port)).value

      assert_equal [[64, "Errno::EINVAL", "Errno::EINVAL", 1, 64],
                    [64, 9, 2304, 4608, "Errno::ENOTSUP", "Errno::ENOPROTOOPT"]], value,
                   "IP_TTL on a TCPSocket and a UDPSocket, and a UDPSocket's buffer sizes, must answer as CRuby's " \
                   "on Linux: 64 by default, EINVAL past 1 to 255, -1 back to 64, and a TCP option on a UDPSocket " \
                   "ENOTSUP to read and ENOPROTOOPT to set"
    end
  end

  UNANSWERED = <<~RUBY
    answer = lambda do |&call|
      call.call
    rescue StandardError, NotImplementedError => e
      "\#{e.class}: \#{e.message}"
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    udp = UDPSocket.new
    answers = [
      answer.call { socket.getsockopt(:TCP, :NODELAY) },
      answer.call { socket.setsockopt(Socket::IPPROTO_TCP, Socket::TCP_NODELAY, 1) },
      answer.call { udp.setsockopt(:SOCKET, :KEEPALIVE, true) },
      answer.call { socket.setsockopt(:SOCKET, :REUSEADDR, 1) },
      answer.call { Socket::Option.int(:UNIX, :SOCKET, :KEEPALIVE, 1) },
      answer.call { Socket::Option.int(:BOGUS, :SOCKET, :KEEPALIVE, 1) }
    ]
    socket.close
    udp.close
    answers
  RUBY

  UNANSWERED_ANSWERS = [
    "Errno::ENOPROTOOPT: Protocol not available - getsockopt(2)",
    "Errno::ENOPROTOOPT: Protocol not available - setsockopt(2)",
    "Errno::ENOPROTOOPT: Protocol not available - setsockopt(2)",
    "SocketError: unknown socket level option name: REUSEADDR",
    "NotImplementedError: address family UNIX is unimplemented: the guest numbers only INET and INET6",
    "SocketError: unknown socket domain: BOGUS"
  ].freeze

  # @behavior NW-127
  def test_an_option_wasi_has_no_call_for_raises_where_cruby_sets_it
    with_upcase_server do |port|
      value = sandbox_reaching(port).eval(format(UNANSWERED, port: port)).value

      assert_equal UNANSWERED_ANSWERS, value,
                   "TCP_NODELAY and a UDPSocket's SO_KEEPALIVE must raise Errno::ENOPROTOOPT, a name the guest " \
                   "defines no constant for SocketError, and a family CRuby knows but the guest cannot number " \
                   "NotImplementedError, where CRuby takes each; a family no one knows raises SocketError as CRuby's"
    end
  end
end
