# frozen_string_literal: true

require "test_helper"

# E2E — how a script's socket reads each argument, as CRuby's converts it,
# through the real Guest Binary against a loopback server; mruby words the
# conversions it raises itself.
class TestE2ENetworkSocketArgsCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  ARGS = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    [
      outcome.call { socket.shutdown(2**40) }.first,
      outcome.call { socket.read(-1) },
      outcome.call { socket.recv(-1) },
      outcome.call { socket.recv(-1, "x") }.first,
      outcome.call { socket.pread },
      outcome.call { socket.seek },
      outcome.call { socket.advise(:normal, "x") }.first,
      socket.addr(false, 1).first,
      outcome.call { socket.gets(:a) }.first
    ]
  RUBY

  # What CRuby 3.4.7 on Linux answers the same script: the class alone where
  # mruby words the conversion, the whole message where both word it alike.
  CRUBY = [
    "RangeError",
    ["ArgumentError", "negative length -1 given"],
    ["ArgumentError", "negative string size (or size too big)"],
    "TypeError",
    ["ArgumentError", "wrong number of arguments (given 0, expected 2..3)"],
    ["ArgumentError", "wrong number of arguments (given 0, expected 1..2)"],
    "TypeError",
    "AF_INET",
    "TypeError"
  ].freeze

  # @behavior NW-191
  def test_a_socket_reads_its_arguments_as_crubys
    with_listening_server do |port|
      value = loopback_sandbox(port).eval(format(ARGS, port: port)).value

      assert_equal CRUBY, value,
                   "a socket must read shutdown's side as NUM2INT, a read's length as io_getpartial and recv's as " \
                   "rsock_strbuf after its flags, check pread's and seek's arity, convert advise's offset, take " \
                   "addr's extra arguments, and read gets' lone Symbol as a limit, as CRuby's do"
    end
  end

  SEPARATOR = <<~RUBY
    separator = Object.new
    def separator.to_str = ","
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("\\r\\n")
    socket.gets(separator)
  RUBY

  # @behavior NW-191
  def test_a_lone_argument_answering_to_str_is_gets_separator
    with_answering_server("a,b\\nc") do |port|
      assert_equal "a,", loopback_sandbox(port).eval(format(SEPARATOR, port: port)).value,
                   "TCPSocket#gets with one object answering to_str must split at what to_str answers, as " \
                   "CRuby's rb_io_getline_args reads it"
    end
  end
end
