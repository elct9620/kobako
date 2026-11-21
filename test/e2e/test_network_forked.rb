# frozen_string_literal: true

require "test_helper"

# E2E — a guest's TCPSocket in a child forked after its parent's guest
# exchanged data, through the real Guest Binary.
#
# The parent's guest connects once before forking, so whatever watches the
# process's host sockets already exists when the child starts. The loopback
# server runs on a Thread of the parent, which the child never inherits, and
# answers each connection with the upcase of what it read.
class TestE2ENetworkForked < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  BOUND = 30

  SCRIPT = <<~RUBY
    require "kobako"
    require "socket"
    server = TCPServer.new("127.0.0.1", 0)
    port = server.addr[1]
    Thread.new do
      loop do
        client = server.accept
        client.write(client.read.upcase)
        client.close
      end
    end
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM")).network(:local) do |net|
      net.endpoint "127.0.0.1", port: port, protocol: :tcp, addresses: [:local]
    end
    ping = format(<<~GUEST, port: port)
      socket = TCPSocket.new("127.0.0.1", %<port>d)
      socket.write("ping")
      socket.close_write
      answer = socket.read
      socket.close
      answer
    GUEST
    sandbox.eval(ping)
    pid = fork do
      begin
        print sandbox.eval(ping).value
      rescue Exception => e
        print e.class
      end
      $stdout.flush
      exit!(0)
    end
    Process.wait(pid)
  RUBY

  def setup
    super
    skip "this platform cannot fork" unless Process.respond_to?(:fork)
  end

  # @behavior NW-237
  def test_a_forked_childs_guest_exchanges_data_as_its_parents_did
    assert_equal "PING", run_ruby(SCRIPT, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: BOUND),
                 "a TCPSocket a guest opens in a child forked after its parent's guest exchanged data " \
                 "must send and read back its bytes as in the parent"
  end
end
