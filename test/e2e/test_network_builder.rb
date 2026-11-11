# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox#network block adding to the object given beside it, through
# the real Guest Binary against a loopback server.
class TestE2ENetworkBuilder < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  CONNECT = <<~RUBY
    begin
      TCPSocket.new("127.0.0.1", %<port>d)
      :connected
    rescue Errno::EACCES
      :refused
    end
  RUBY

  # An entry naming one loopback TCP endpoint, whose own connect? refuses
  # every connect while +refusing+.
  Given = Struct.new(:port, :refusing) do
    def endpoints
      [Kobako::Network::Endpoint.new(host: "127.0.0.1", port: port, protocol: :tcp, addresses: [:local])]
    end

    def connect?(_connection) = !refusing
  end

  # Connect from a Sandbox given Given and a block whose connect policy
  # answers +block_allows+, then once from the host so the server's accept
  # returns whatever the guest did.
  def connect_through(port, refusing:, block_allows:)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:given, Given.new(port, refusing)) do |net|
      net.policy(:connect) { block_allows }
    end
    value = sandbox.eval(format(CONNECT, port: port)).value
    TCPSocket.new("127.0.0.1", port).close
    value
  end

  # @behavior NW-020
  def test_a_block_adds_its_policy_to_the_object_given_beside_it
    with_upcase_server do |port|
      answers = [[false, true], [true, true], [false, false]].map do |refusing, block_allows|
        connect_through(port, refusing: refusing, block_allows: block_allows)
      end

      assert_equal %i[connected refused refused], answers,
                   "Sandbox#network given an object and a block must reach the object's endpoints and refuse " \
                   "what either the object's connect? or the block's policy(:connect) refuses"
    end
  end
end
