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

  PROBES_AND_BUFFERS = <<~RUBY
    refused = lambda do |&call|
      call.call
      :taken
    rescue Errno::EINVAL
      :einval
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    defaults = [socket.getsockopt(:TCP, :KEEPIDLE).int, socket.getsockopt(:TCP, :KEEPINTVL).int,
                socket.getsockopt(:TCP, :KEEPCNT).int, socket.getsockopt(:SOCKET, :RCVBUF).int,
                socket.getsockopt(:SOCKET, :SNDBUF).int]
    bounds = [
      refused.call { socket.setsockopt(:TCP, :KEEPIDLE, 0) },
      refused.call { socket.setsockopt(:TCP, :KEEPIDLE, 32_768) },
      refused.call { socket.setsockopt(:TCP, :KEEPINTVL, 0) },
      refused.call { socket.setsockopt(:TCP, :KEEPCNT, 0) },
      refused.call { socket.setsockopt(:TCP, :KEEPCNT, 128) }
    ]
    socket.setsockopt(:TCP, :KEEPIDLE, 32_767)
    socket.setsockopt(Socket::IPPROTO_TCP, Socket::TCP_KEEPINTVL, 10)
    socket.setsockopt(:TCP, :KEEPCNT, 127)
    probes = [socket.getsockopt(:TCP, :KEEPIDLE).inspect, socket.getsockopt(:TCP, :KEEPINTVL).int,
              socket.getsockopt(:TCP, :KEEPCNT).int]
    sizes = [1000, 0, -1, 10_000_000].map do |size|
      socket.setsockopt(:SOCKET, :RCVBUF, size)
      socket.getsockopt(:SOCKET, :RCVBUF).int
    end
    socket.setsockopt(:SOCKET, :SNDBUF, 1000)
    sizes << socket.getsockopt(:SOCKET, :SNDBUF).inspect
    socket.close
    [defaults, bounds, probes, sizes]
  RUBY

  # @behavior NW-124
  def test_a_script_tunes_keepalive_probes_and_buffers_as_linux_takes_them
    with_upcase_server do |port|
      value = loopback_sandbox(port).eval(format(PROBES_AND_BUFFERS, port: port)).value

      assert_equal [[7200, 75, 9, 65_536, 65_536], %i[einval einval einval einval einval],
                    # mruby's String#inspect spells hex lowercase, where CRuby's is uppercase.
                    ["#<Socket::Option: INET TCP KEEPIDLE \"\\xff\\x7f\\x00\\x00\">", 10, 127],
                    [2304, 2304, 65_536, 65_536, "#<Socket::Option: INET SOCKET SNDBUF 4608>"]], value,
                   "TCPSocket's keepalive options must read Linux's defaults, refuse with Errno::EINVAL what " \
                   "Linux refuses, and read back what was set; SO_RCVBUF and SO_SNDBUF must read twice the size " \
                   "set, above Linux's least and within the socket's own buffer"
    end
  end

  # @behavior NW-123
  def test_a_script_sets_and_reads_keepalive_as_cruby_does
    with_upcase_server do |port|
      value = loopback_sandbox(port).eval(format(KEEPALIVE, port: port)).value

      assert_equal KEEPALIVE_ANSWERS, value,
                   "TCPSocket#setsockopt and #getsockopt of SO_KEEPALIVE must answer as CRuby's on Linux: 0 from " \
                   "a set, a Socket::Option reading back each form set, and the same errors for a short value, an " \
                   "unknown name or level, a nil value, and a closed socket"
    end
  end
end
