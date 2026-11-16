# frozen_string_literal: true

require "test_helper"

# E2E — a script connecting with Socket.tcp, through the real Guest Binary
# against a loopback server: the socket it answers, the block form, a
# refusal, and a lookup's deadline.
class TestE2ENetworkSocketTcp < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  CONNECTS = <<~RUBY
    plain = Socket.tcp("127.0.0.1", %<open>d)
    held = nil
    answered = Socket.tcp("127.0.0.1", %<open>d) do |socket|
      held = socket
      socket.closed?
    end
    refused = begin
      Socket.tcp("127.0.0.1", %<closed>d)
    rescue SystemCallError => e
      [e.class.to_s, e.message]
    end
    past = begin
      Socket.tcp("unnamed.example", %<open>d, resolv_timeout: -1)
    rescue StandardError => e
      e.class.to_s
    end
    stalled = [0.2, -1].map do |sec|
      Socket.tcp("stall.test", %<open>d, resolv_timeout: sec)
    rescue SystemCallError => e
      [e.class.to_s, e.message]
    end
    [[plain.class.to_s, plain.closed?, plain.peeraddr[1]], answered, held.closed?, refused, past, stalled]
  RUBY

  # What CRuby's Socket.tcp raises for a lookup still waiting at its
  # resolv_timeout: deadline.
  TIMED_OUT = ["Errno::ETIMEDOUT", "Connection timed out - user specified timeout"].freeze

  # What CONNECTS answers in a Sandbox reaching loopback at +open+ and
  # +closed+, and `stall.test` at +open+, through a Mock that routes +open+
  # alone, so a dial to +closed+ is refused whatever listens there on the
  # host, and holds each lookup of `stall.test` until the run ends.
  def connects(open, closed)
    mock = mock_resolving({ "stall.test" => "127.0.0.1" })
    mock.route("127.0.0.1", open, to: ["127.0.0.1", open])
    sandbox = loopback_sandbox(open, closed, network: mock, timeout: 10).network(:stall) do |net|
      net.endpoint "stall.test", port: open, protocol: :tcp, addresses: [:local]
    end
    mock.hold("stall.test") { sandbox.eval(format(CONNECTS, open: open, closed: closed)).value }
  end

  # @behavior NW-143
  def test_a_script_connects_with_socket_tcp_as_crubys
    with_listening_server do |open|
      closed = other_port(open)
      assert_equal [["TCPSocket", false, open], false, true,
                    ["Errno::ECONNREFUSED", "Connection refused - connect(2) for 127.0.0.1:#{closed}"],
                    "Socket::ResolutionError", [TIMED_OUT, TIMED_OUT]], connects(open, closed),
                   "Socket.tcp must answer a connected TCPSocket, close the block's socket after it and answer the " \
                   "block's value, name a refused end as host:port, and read resolv_timeout: as a deadline: a " \
                   "lookup still waiting at it raises Errno::ETIMEDOUT, and a negative one is already past, so " \
                   "a lookup answered at once still answers"
    end
  end
end
