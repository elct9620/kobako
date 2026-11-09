# frozen_string_literal: true

require "test_helper"

# E2E — a script shutting its socket's sides, through the real Guest Binary
# against loopback peers: shutdown by name and number, what reads and writes
# after each side shuts, close_read beside close_write, and a closed socket.
class TestE2ENetworkSocketShutdown < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback
  include UdpLoopback

  OUTCOME = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
    open = lambda do
      socket = TCPSocket.new("127.0.0.1", %<tcp>d)
      IO.select([socket])
      socket
    end
  RUBY

  TCP_SIDES = <<~RUBY.freeze
    #{OUTCOME}
    named = open.call
    shuts = [
      named.shutdown(:SHUT_WR), named.shutdown("RD"), named.shutdown(0), named.shutdown, named.shutdown(nil),
      outcome.call { named.shutdown(:FOO) }, outcome.call { named.shutdown(5) }
    ]
    sides = open.call
    sides.shutdown(Socket::SHUT_RD)
    arrived = sides.read
    sides.shutdown(Socket::SHUT_WR)
    broken = outcome.call { sides.write("x") }
    halves = open.call
    line = halves.gets
    halves.close_read
    refused = [outcome.call { halves.read }, halves.closed?]
    halves.close_write
    closed = open.call
    closed.gets
    closed.close
    after = [outcome.call { closed.getc }, outcome.call { closed.read }, outcome.call { closed.recv(1) }]
    [shuts, arrived, broken, line, refused, halves.closed?, after]
  RUBY

  TCP_ANSWERS = [
    [0, 0, 0, 0, 0, ["SocketError", "unknown shutdown argument: FOO"],
     ["ArgumentError", "`how' should be either :SHUT_RD, :SHUT_WR, :SHUT_RDWR"]],
    "a\nb",
    ["Errno::EPIPE", "Broken pipe"],
    "a\n",
    [["IOError", "not opened for reading"], false],
    true,
    [["IOError", "closed stream"]] * 3
  ].freeze

  # @behavior NW-136
  def test_a_script_shuts_a_tcp_sockets_sides_as_crubys
    with_lingering_server do |tcp|
      value = sandbox_reaching(tcp).eval(format(TCP_SIDES, tcp: tcp)).value

      assert_equal TCP_ANSWERS, value,
                   "TCPSocket#shutdown must answer 0 for each name and number CRuby takes and raise as CRuby's for " \
                   "the rest; a write after SHUT_WR must raise Errno::EPIPE and what arrived still read after " \
                   "SHUT_RD; #close_read must refuse reads and close the socket with #close_write; and a closed " \
                   "socket must raise IOError on every read though a #gets held bytes back"
    end
  end

  UDP_SIDES = <<~RUBY.freeze
    #{OUTCOME}
    socket = UDPSocket.new
    unconnected = outcome.call { socket.shutdown }
    socket.connect("127.0.0.1", %<udp>d)
    [unconnected, socket.shutdown(:WR), outcome.call { socket.send("x", 0) }, outcome.call { socket.write("x") }]
  RUBY

  # @behavior NW-137
  def test_a_script_shuts_a_udp_sockets_sending_side
    with_quiet_udp_peer do |udp|
      value = sandbox_sending_to(udp).eval(format(UDP_SIDES, tcp: 0, udp: udp)).value

      assert_equal [["Errno::ENOTCONN", "Transport endpoint is not connected - shutdown(2)"], 0,
                    ["Errno::EPIPE", "Broken pipe - send(2)"], ["Errno::EPIPE", "Broken pipe"]], value,
                   "UDPSocket#shutdown before #connect must raise Errno::ENOTCONN, and after it #shutdown(:WR) must " \
                   "answer 0 and a later #send or #write raise Errno::EPIPE, as Linux's datagram socket does"
    end
  end

  # Yield the port of a loopback server answering each connection with
  # "a\nb", then reading it to its end.
  def with_lingering_server
    server = TCPServer.new("127.0.0.1", 0)
    thread = Thread.new { loop { Thread.new(server.accept) { |c| c.write("a\nb") && c.read && c.close } } }
    yield server.addr[1]
  ensure
    thread&.kill
    server&.close
  end
end
