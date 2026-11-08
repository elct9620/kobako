# frozen_string_literal: true

require "test_helper"

# E2E — a script's TCPSocket through the real Guest Binary against a loopback
# server: what an endpoint admits, and what a connect policy refuses.
class TestE2ENetworkTcp < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  PING = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("ping")
    socket.close_write
    answer = socket.read
    socket.close
    answer
  RUBY

  CONNECT = <<~RUBY
    begin
      TCPSocket.new("127.0.0.1", %<port>d)
      :connected
    rescue Errno::EACCES
      :refused
    end
  RUBY

  # Run CONNECT in +sandbox+ to +port+, then connect once from the host so the
  # server's accept returns whatever the guest did.
  def connect_from(sandbox, port)
    value = sandbox.eval(format(CONNECT, port: port)).value
    TCPSocket.new("127.0.0.1", port).close
    value
  end

  # @behavior NW-016
  def test_a_tcp_socket_reaches_a_loopback_server_an_endpoint_names
    with_upcase_server do |port|
      value = sandbox_reaching(port).eval(format(PING, port: port)).value

      assert_equal "PING", value,
                   "TCPSocket to a loopback server an endpoint names must send and read back its bytes"
    end
  end

  # @behavior NW-017
  def test_a_tcp_socket_to_an_address_no_endpoint_names_raises_eacces
    with_upcase_server do |port|
      assert_equal :refused, connect_from(sandbox_reaching(port + 1), port),
                   "TCPSocket to a port no endpoint names must raise Errno::EACCES"
    end
  end

  SELECT = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("ping")
    silent = IO.select([socket], nil, nil, 0.05)
    socket.close_write
    answered = IO.select([socket], nil, nil, 5)
    writable = IO.select(nil, [$stdout, socket], nil, 5)
    [
      [IPSocket, BasicSocket, IO].all? { |ancestor| socket.is_a?(ancestor) },
      socket.fileno.is_a?(Integer),
      silent.nil?,
      answered && answered[0] == [socket],
      writable && writable[1] == [$stdout, socket],
      socket.read
    ]
  RUBY

  # @behavior NW-065
  def test_a_tcp_socket_is_an_io_that_select_waits_on
    with_upcase_server do |port|
      value = sandbox_reaching(port).eval(format(SELECT, port: port)).value

      assert_equal [true, true, true, true, true, "PING"], value,
                   "TCPSocket must descend from IPSocket, BasicSocket, and IO with an Integer fileno, and IO.select " \
                   "must time out with nil on a silent socket, answer it readable once the server writes, and " \
                   "answer $stdout beside it writable"
    end
  end

  SECOND = <<~RUBY
    TCPSocket.new("127.0.0.1", %<port>d).close
    begin
      TCPSocket.new("127.0.0.1", %<port>d)
      :connected
    rescue Errno::EAGAIN
      :refused
    end
  RUBY

  # @behavior NW-059
  def test_a_connection_past_the_runs_connection_limit_raises_eagain
    with_upcase_server do |port|
      value = sandbox_reaching(port, connection_limit: 1).eval(format(SECOND, port: port)).value

      assert_equal :refused, value,
                   "a second TCPSocket in a run under connection_limit: 1 must raise Errno::EAGAIN, even once the " \
                   "first closed"
    end
  end

  # An entry playing only a policy, recording each Connection it is handed.
  class Recorder
    attr_reader :seen

    def initialize
      @seen = []
    end

    def connect?(connection)
      @seen << connection
      true
    end
  end

  # @behavior NW-021
  def test_a_connect_policy_block_refuses_a_tcp_socket_the_endpoints_allow
    with_upcase_server do |port|
      sandbox = sandbox_reaching(port).network(:guard) { |net| net.policy(:connect) { |c| c.port != port } }

      assert_equal :refused, connect_from(sandbox, port),
                   "TCPSocket a net.policy(:connect) block refuses must raise Errno::EACCES"
    end
  end

  # @behavior NW-021
  def test_an_entry_connect_policy_is_handed_the_connection_the_guest_named
    recorder = Recorder.new
    with_upcase_server do |port|
      sandbox_reaching(port).network(:audit, recorder).eval(format(PING, port: port))

      assert_equal [Kobako::Network::Connection.new(host: "127.0.0.1", port: port, protocol: :tcp)],
                   recorder.seen,
                   "an entry's connect? must be handed a Connection naming the host, port, and :tcp the guest named"
    end
  end
end
