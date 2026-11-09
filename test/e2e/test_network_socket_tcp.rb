# frozen_string_literal: true

require "test_helper"

# E2E — a script connecting with Socket.tcp, through the real Guest Binary
# against a loopback server: the socket it answers, the block form, and a
# refusal.
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
    [[plain.class.to_s, plain.closed?, plain.peeraddr[1]], answered, held.closed?, refused]
  RUBY

  # @behavior NW-143
  def test_a_script_connects_with_socket_tcp_as_crubys
    with_listening_server do |open|
      closed = port_besides(open)
      value = sandbox_reaching_ports(open, closed).eval(format(CONNECTS, open: open, closed: closed)).value

      assert_equal [["TCPSocket", false, open], false, true,
                    ["Errno::ECONNREFUSED", "Connection refused - connect(2) for 127.0.0.1:#{closed}"]], value,
                   "Socket.tcp must answer a connected TCPSocket, close the block's socket after it and answer the " \
                   "block's value, and name a refused end as host:port"
    end
  end

  def sandbox_reaching_ports(*ports)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:local) do |net|
      ports.each { |port| net.endpoint "127.0.0.1", port: port, protocol: :tcp, addresses: [:local] }
    end
  end
end
