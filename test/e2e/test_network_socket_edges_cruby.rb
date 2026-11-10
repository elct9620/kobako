# frozen_string_literal: true

require "test_helper"

# E2E — a script's socket at its read and write edges, through the real Guest
# Binary against loopback peers: a zero-length receive, paragraphs, an outbuf
# at the end, and what a closed side still lets through.
class TestE2ENetworkSocketEdgesCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback
  include UdpLoopback

  EDGES = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue IOError, SystemCallError => e
      [e.class.to_s, e.message]
    end
    tcp = TCPSocket.new("127.0.0.1", %<tcp>d)
    tcp.write("\\r\\n")
    IO.select([tcp])
    read = [tcp.recv(0), tcp.gets(""), tcp.read]
    partial = +"keep"
    received = +"keep"
    ended = [outcome.call { tcp.readpartial(5, partial) }, partial, tcp.recv(5, 0, received), received]
    late = TCPSocket.new("127.0.0.1", %<late>d)
    waiting = outcome.call { late.recv_nonblock(0) }
    late.write("\\r\\n")
    IO.select([late])
    late.close_read
    closed = [waiting, late.recv(5), outcome.call { late.wait_readable(0) }]
    udp = UDPSocket.new
    udp.connect("127.0.0.1", %<udp>d)
    udp.send("x", 0)
    IO.select([udp])
    datagrams = [udp.recv(0), udp.recv(10)]
    udp.close_write
    [read, ended, closed, datagrams, outcome.call { udp.send("x", 0) }]
  RUBY

  ANSWERS = [[nil, "ab\n\n", "cd"], [["EOFError", "end of file reached"], "", nil, "keep"],
             [["IO::EAGAINWaitReadable", "Resource temporarily unavailable - recvfrom(2) would block"], "hello",
              ["IOError", "not opened for reading"]], ["", "two"],
             ["Errno::EPIPE", "Broken pipe - send(2)"]].freeze

  # @behavior NW-144
  def test_a_sockets_read_and_write_edges_answer_as_crubys
    with_answering_server("ab\n\n\n\ncd") do |tcp|
      with_answering_server("hello") do |late|
        with_two_datagram_peer do |udp|
          value = tcp_and_udp_sandbox(tcp, udp).network(:late) { |net| add_loopback_tcp(net, late) }
                                               .eval(format(EDGES, tcp: tcp, late: late, udp: udp)).value

          assert_equal ANSWERS, value, EDGES_PROMISE
        end
      end
    end
  end

  EDGES_PROMISE = "TCPSocket#recv(0) must answer nil and a UDPSocket's drop a datagram; a paragraph must take the " \
                  "newlines after it; at the end #readpartial must empty its outbuf and #recv keep it; " \
                  "#recv_nonblock(0) before bytes come must raise IO::EAGAINWaitReadable; after #close_read #recv " \
                  "must read what came and #wait_readable raise IOError; a send after #close_write EPIPE"

  # Yield the port of a loopback peer answering a datagram with "one", then
  # "two".
  def with_two_datagram_peer
    peer = UDPSocket.new
    peer.bind("127.0.0.1", 0)
    thread = Thread.new do
      _, from = peer.recvfrom(16)
      %w[one two].each { |mesg| peer.send(mesg, 0, from[3], from[1]) }
    end
    yield peer.addr[1]
  ensure
    thread&.kill
    peer&.close
  end
end
