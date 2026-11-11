# frozen_string_literal: true

require "test_helper"

# E2E — a script's TCPSocket#send, through the real Guest Binary against a
# loopback server: the flags and destination it takes, the values it refuses,
# and its refusals once the writing side or the socket is closed.
class TestE2ENetworkTcpSend < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  SENDS = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    sent = [socket.send("hi", 0), socket.send("x", 64), socket.send("v", 64.7),
            socket.send("y", 0, Socket.sockaddr_in(%<port>d, "127.0.0.1"))]
    refused = [outcome.call { socket.send("z", 1) }, outcome.call { socket.send("z", nil) },
               outcome.call { socket.send(123, 0) }, outcome.call { socket.send("z", 0, 5) },
               outcome.call { socket.send("z", Float::NAN) }].map(&:first)
    socket.close_write
    shut = outcome.call { socket.send("w", 0) }
    answer = socket.read
    socket.close
    [sent, refused, shut, answer, outcome.call { socket.send("w", 0) }.first]
  RUBY

  OUTCOMES = [[2, 1, 1, 1], %w[Errno::ENOTSUP TypeError TypeError TypeError RangeError],
              ["Errno::EPIPE", "Broken pipe - send(2)"], "HIXVY", "IOError"].freeze

  # @behavior NW-180
  def test_a_tcp_socket_sends_as_crubys_basic_socket
    with_upcase_server do |port|
      value = loopback_sandbox(port).eval(format(SENDS, port: port)).value

      assert_equal OUTCOMES, value,
                   "TCPSocket#send must answer the bytes sent with flags 0, MSG_DONTWAIT's 64, 64.7, and a " \
                   "destination, which the server reads in order; MSG_OOB's 1 must raise Errno::ENOTSUP, since WASI " \
                   "carries no send flag; nil and an Integer message or destination TypeError, NaN flags " \
                   "RangeError, a send after " \
                   "#close_write Errno::EPIPE naming send(2), and one on a closed socket IOError, as CRuby's do"
    end
  end
end
