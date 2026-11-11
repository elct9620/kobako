# frozen_string_literal: true

require "test_helper"
require "socket"

# E2E — the flags a script's UDPSocket#send takes, through the real Guest
# Binary against a loopback peer: the one WASI carries sends, another raises.
class TestE2ENetworkUdpFlags < Minitest::Test
  include E2eGuestHelper
  include UdpLoopback

  SEND_FLAGS = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      e.class.to_s
    end
    socket = UDPSocket.new
    socket.connect("127.0.0.1", %<port>d)
    [
      outcome.call { socket.send("a", 64) },
      outcome.call { socket.send("x", 1) },
      outcome.call { socket.send("x", nil, "127.0.0.1", %<port>d) }
    ]
  RUBY

  SEND_DESTS = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      e.class.to_s
    end
    socket = UDPSocket.new
    socket.connect("127.0.0.1", %<port>d)
    [
      outcome.call { socket.send("a", 0, Socket.sockaddr_in(%<port>d, "127.0.0.1")) },
      outcome.call { socket.send("b", 0, Addrinfo.udp("127.0.0.1", %<port>d)) },
      outcome.call { socket.send("c", 0, Socket.sockaddr_in(%<port>d ^ 1, "127.0.0.1")) },
      outcome.call { socket.send("d", 0, 5) }, outcome.call { socket.send(5, 0) }
    ]
  RUBY

  # @behavior NW-183
  def test_a_send_names_its_peer_by_sockaddr_or_addrinfo
    with_hello_peer do |port|
      value = udp_sandbox(port).eval(format(SEND_DESTS, port: port)).value

      assert_equal [1, 1, "Errno::EACCES", "TypeError", "TypeError"], value,
                   "UDPSocket#send naming the peer by a sockaddr String or an Addrinfo must send, as CRuby's " \
                   "sendto takes either; one naming another port must raise Errno::EACCES, and an Integer " \
                   "destination or message TypeError"
    end
  end

  # @behavior NW-176
  def test_a_send_flag_wasi_cannot_carry_raises
    with_upcase_peer do |port|
      value = udp_sandbox(port).eval(format(SEND_FLAGS, port: port)).value

      assert_equal [1, "Errno::ENOTSUP", "TypeError"], value,
                   "UDPSocket#send with MSG_DONTWAIT's 64 must send, as CRuby's does, while MSG_OOB's 1 raises " \
                   "Errno::ENOTSUP and a nil flag TypeError, as CRuby converts flags with NUM2INT"
    end
  end
end
