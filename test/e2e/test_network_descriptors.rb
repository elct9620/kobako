# frozen_string_literal: true

require "test_helper"

# E2E — the host descriptors a guest's connection holds, against a process
# with one left, through the real Guest Binary. The loopback server runs in
# this process, and each child connects once before it fills, so the reactor
# it starts on the first connection holds its own already and the one left
# is the connection's alone.
class TestE2ENetworkDescriptors < Minitest::Test
  include DescriptorChild
  include NetworkLoopback

  TCP = <<~RUBY
    sandboxed = Kobako::Sandbox.new(wasm_path: WASM).network(:local) do |net|
      net.endpoint "127.0.0.1", port: %<port>d, protocol: :tcp, addresses: [:local]
    end
    ping = 'socket = TCPSocket.new("127.0.0.1", %<port>d); socket.write("ping"); socket.close_write; socket.read'
    sandboxed.eval(ping)
    leave(1)
    print answer(sandboxed, ping)
  RUBY

  HTTP = <<~RUBY
    sandboxed = Kobako::Sandbox.new(wasm_path: WASM).network(:web) do |net|
      net.endpoint "127.0.0.1", port: %<port>d
    end
    get = 'Net::HTTP.get("127.0.0.1", "/", %<port>d)'
    sandboxed.eval(get)
    leave(1)
    print answer(sandboxed, get)
  RUBY

  # A loopback server answering each connection with the upcase of what it
  # read, as many as come.
  def with_upcasing_server
    server = TCPServer.new("127.0.0.1", 0)
    thread = Thread.new { loop { upcase_one(server.accept) } }
    yield server.addr[1]
  ensure
    thread&.kill
    server&.close
  end

  def upcase_one(client)
    client.write(client.read.upcase)
    client.close
  end

  # @behavior NW-248
  def test_a_connection_holds_one_host_descriptor
    tcp = with_upcasing_server { |port| run_descriptor_child(format(TCP, port: port)) }
    http = with_serving_server { |port, _seen| run_descriptor_child(format(HTTP, port: port)) }
    assert_equal %w[PING ok], [tcp, http],
                 "TCPSocket and Net::HTTP to a loopback server, with one host descriptor left, must each land"
  end
end
