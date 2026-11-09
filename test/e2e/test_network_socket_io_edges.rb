# frozen_string_literal: true

require "test_helper"

# E2E — a script's socket at IO's edge calls, through the real Guest Binary
# against a loopback server: zero lengths and limits, paragraphs, advice, and
# a write after the write side closed.
class TestE2ENetworkSocketIoEdges < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  EDGES = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError, NotImplementedError => e
      [e.class.to_s, e.message]
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("\\r\\n")
    reads = [socket.readpartial(0), socket.gets(0), socket.gets(""), socket.gets("")]
    refusals = [outcome.call { socket.readlines(0) }, outcome.call { socket.each_line("\\n", 0) { nil } },
                outcome.call { socket.advise(:bogus) }.first, outcome.call { socket.advise(1) }.first]
    socket.close_write
    reads + refusals + [outcome.call { socket.write("x") }]
  RUBY

  EDGES_IN_CRUBY = ["", "", "a\n\n", "b\nhello",
                    ["ArgumentError", "invalid limit: 0 for readlines"],
                    ["ArgumentError", "invalid limit: 0 for each_line"],
                    "NotImplementedError", "TypeError", ["IOError", "not opened for writing"]].freeze

  # @behavior NW-118
  def test_a_tcp_socket_answers_ios_edge_calls_as_crubys
    with_answering_server("a\n\n\nb\nhello") do |port|
      assert_equal EDGES_IN_CRUBY, sandbox_reaching(port).eval(format(EDGES, port: port)).value,
                   "TCPSocket#readpartial(0) and #gets(0) must answer \"\", #gets(\"\") read paragraphs, " \
                   "#readlines(0) and #each_line(sep, 0) raise ArgumentError, #advise of a bad advice raise " \
                   "NotImplementedError or TypeError, and #write after #close_write raise IOError, as CRuby's IO does"
    end
  end
end
