# frozen_string_literal: true

require "test_helper"

# E2E — a script's TCPSocket once both ends finished, through the real Guest
# Binary against a loopback peer: what shutdown and the peer's name answer
# after the socket sent its end and read its peer's.
class TestE2ENetworkSocketFinished < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  FINISHED = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
    socket = TCPSocket.new("127.0.0.1", %<tcp>d)
    socket.close_write
    socket.read
    [outcome.call { socket.shutdown(:RD) }, outcome.call { socket.shutdown(:WR) }, outcome.call { socket.shutdown },
     outcome.call { socket.getpeername }, outcome.call { socket.peeraddr }, outcome.call { socket.remote_address },
     socket.read, socket.addr[0]]
  RUBY

  ANSWERS = [
    *[["Errno::ENOTCONN", "Transport endpoint is not connected - shutdown(2)"]] * 3,
    *[["Errno::ENOTCONN", "Transport endpoint is not connected - getpeername(2)"]] * 3,
    "", "AF_INET"
  ].freeze

  # @behavior NW-222
  def test_a_tcp_socket_both_ends_finished_answers_enotconn_as_crubys
    with_upcase_server do |tcp|
      value = loopback_sandbox(tcp).eval(format(FINISHED, tcp: tcp)).value

      assert_equal ANSWERS, value,
                   "after #close_write and a read to the peer's end, TCPSocket#shutdown by any side, " \
                   "#getpeername, #peeraddr, and #remote_address must raise Errno::ENOTCONN, as CRuby's do " \
                   "on Linux, while a read still answers the end and #addr the socket's own end"
    end
  end
end
