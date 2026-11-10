# frozen_string_literal: true

require "test_helper"

# E2E — a script's UDPSocket received without waiting, through the real Guest
# Binary against a loopback peer: what answers before a datagram comes, and
# the datagram once it does.
class TestE2ENetworkUdpNonblock < Minitest::Test
  include E2eGuestHelper
  include UdpLoopback

  DATAGRAMS = <<~RUBY.freeze
    #{NonblockOutcome::SCRIPT}
    socket = UDPSocket.new
    socket.connect("127.0.0.1", %<port>d)
    before = [
      outcome.call { socket.recvfrom_nonblock(10) }, socket.recvfrom_nonblock(10, exception: false),
      outcome.call { socket.recv_nonblock(10) }, outcome.call { socket.read_nonblock(10) }
    ]
    socket.send("one", 0)
    IO.select([socket])
    cut = socket.recvfrom_nonblock(3)
    socket.send("two", 0)
    IO.select([socket])
    [before, cut, socket.read_nonblock(10)]
  RUBY

  EARLY_READS = [
    NonblockOutcome.would_block("recvfrom(2) would block"), :wait_readable,
    NonblockOutcome.would_block("recvfrom(2) would block"), NonblockOutcome.would_block("read would block")
  ].freeze

  # @behavior NW-131
  def test_a_udp_socket_receives_without_waiting_as_crubys
    with_hello_peer do |port|
      value = udp_sandbox(port).eval(format(DATAGRAMS, port: port)).value

      assert_equal [EARLY_READS, ["hel", ["AF_INET", port, "127.0.0.1", "127.0.0.1"]], "hello"], value,
                   "UDPSocket#recvfrom_nonblock, #recv_nonblock, and #read_nonblock before a datagram comes must " \
                   "raise IO::EAGAINWaitReadable or answer :wait_readable with exception: false; after one comes " \
                   "#recvfrom_nonblock(3) must answer it cut with its sender and #read_nonblock it whole"
    end
  end
end
