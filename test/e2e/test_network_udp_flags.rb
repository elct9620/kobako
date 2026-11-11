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
