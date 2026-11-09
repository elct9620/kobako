# frozen_string_literal: true

require "test_helper"

# E2E — the socket options a script sets and reads, through the real Guest
# Binary against a loopback server, each answering as CRuby's on Linux.
class TestE2ENetworkSocketOptions < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  KEEPALIVE = <<~RUBY
    failed = lambda do |&call|
      call.call
    rescue StandardError => e
      "\#{e.class}: \#{e.message}"
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    before = socket.getsockopt(:SOCKET, :KEEPALIVE).inspect
    set = socket.setsockopt(:SOCKET, :KEEPALIVE, true)
    after = socket.getsockopt(Socket::SOL_SOCKET, Socket::SO_KEEPALIVE)
    socket.setsockopt("SOCKET", "KEEPALIVE", "\\x00\\x00\\x00\\x00")
    string = socket.getsockopt(:SOL_SOCKET, :SO_KEEPALIVE).bool
    socket.setsockopt(Socket::Option.bool(:INET, :SOCKET, :KEEPALIVE, true))
    object = socket.getsockopt(:SOCKET, :KEEPALIVE).int
    socket.setsockopt(:SOCKET, :KEEPALIVE, 2)
    normalized = socket.getsockopt(:SOCKET, :KEEPALIVE).int
    errors = [
      failed.call { socket.setsockopt(:SOCKET, :KEEPALIVE, "a") },
      failed.call { socket.setsockopt(:SOCKET, :BOGUS, 1) },
      failed.call { socket.setsockopt(:BOGUS, :KEEPALIVE, 1) },
      failed.call { socket.setsockopt(Socket::SOL_SOCKET, 999, 1) },
      failed.call { socket.getsockopt(99, 1) },
      failed.call { socket.setsockopt(:SOCKET, :KEEPALIVE, nil) }
    ]
    socket.close
    errors << failed.call { socket.setsockopt(:SOCKET, :KEEPALIVE, true) }
    [before, set, after.inspect, after.int, after.bool, after.data, after.level, after.optname, after.family,
     string, object, normalized, errors]
  RUBY

  # What CRuby 3.4 on Linux answers for KEEPALIVE.
  KEEPALIVE_ANSWERS = [
    "#<Socket::Option: INET SOCKET KEEPALIVE 0>", 0, "#<Socket::Option: INET SOCKET KEEPALIVE 1>",
    1, true, "\x01\x00\x00\x00", 1, 9, 2, false, 1, 1,
    ["Errno::EINVAL: Invalid argument - setsockopt(2)",
     "SocketError: unknown socket level option name: BOGUS",
     "SocketError: unknown protocol level: BOGUS",
     "Errno::ENOPROTOOPT: Protocol not available - setsockopt(2)",
     "Errno::ENOTSUP: Operation not supported - getsockopt(2)",
     "TypeError: no implicit conversion of nil into String",
     "IOError: closed stream"]
  ].freeze

  # @behavior NW-123
  def test_a_script_sets_and_reads_keepalive_as_cruby_does
    with_upcase_server do |port|
      value = sandbox_reaching(port).eval(format(KEEPALIVE, port: port)).value

      assert_equal KEEPALIVE_ANSWERS, value,
                   "TCPSocket#setsockopt and #getsockopt of SO_KEEPALIVE must answer as CRuby's on Linux: 0 from " \
                   "a set, a Socket::Option reading back each form set, and the same errors for a short value, an " \
                   "unknown name or level, a nil value, and a closed socket"
    end
  end
end
