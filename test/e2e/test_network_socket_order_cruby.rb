# frozen_string_literal: true

require "test_helper"

# E2E — the order a script's socket checks a call in, as CRuby's does, through
# the real Guest Binary against a loopback server: a buffer converted before
# any byte is taken, a side closed whatever its shutdown answers, an empty
# write, and a closed socket.
class TestE2ENetworkSocketOrderCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  ORDER = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      e.class.to_s
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("\\r\\n")
    socket.wait_readable
    kept = [outcome.call { socket.recv(1, 0, 5) }, outcome.call { socket.read(1, 5) }, socket.recv(6)]
    udp = UDPSocket.new
    sides = [outcome.call { udp.close_read }, outcome.call { udp.close_write }, udp.closed?]
    lone = UDPSocket.new
    empty = [lone.write(""), lone.write, outcome.call { lone.syswrite("") }]
    lone.close
    closed = [
      lone.write(""), lone.sysread(0), outcome.call { lone.readpartial(0) },
      outcome.call { lone.read_nonblock(0) }, outcome.call { lone.sync }, outcome.call { lone.recv(-1) }
    ]
    [kept, sides, empty, closed]
  RUBY

  # What CRuby 3.4.7 on Linux answers the same script.
  CRUBY = [
    %w[TypeError TypeError abcdef],
    [nil, nil, true],
    [0, 0, "Errno::EDESTADDRREQ"],
    [0, "", "IOError", "IOError", "IOError", "ArgumentError"]
  ].freeze

  # @behavior NW-189
  def test_a_socket_checks_a_call_in_crubys_order
    with_answering_server("abcdef") do |port|
      value = loopback_sandbox(port).eval(format(ORDER, port: port)).value

      assert_equal CRUBY, value,
                   "a socket must convert a read's buffer before taking a byte, close a side whatever its " \
                   "shutdown answers, answer 0 to an empty write, and on a closed socket answer sysread(0) and " \
                   "refuse readpartial, read_nonblock, and sync with IOError, and recv(-1) with ArgumentError, " \
                   "as CRuby's do"
    end
  end
end
