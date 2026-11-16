# frozen_string_literal: true

require "test_helper"

# E2E — the Socket constants a script names and the waits it makes, as
# CRuby's on Linux, through the real Guest Binary: each constant on Socket
# and on Socket::Constants, IO#wait's modes, and IPSocket.getaddress.
class TestE2ENetworkSocketConstantsCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  NAMES = %w[MSG_DONTWAIT AF_UNSPEC PF_UNSPEC PF_INET PF_INET6 INADDR_ANY INADDR_LOOPBACK INADDR_BROADCAST
             INADDR_NONE AI_NUMERICSERV EAI_AGAIN EAI_BADFLAGS EAI_FAIL EAI_FAMILY EAI_MEMORY EAI_NONAME
             EAI_SERVICE EAI_SOCKTYPE EAI_SYSTEM EAI_ADDRFAMILY EAI_OVERFLOW NI_NOFQDN NI_NAMEREQD NI_DGRAM
             NI_MAXHOST NI_MAXSERV AF_INET SOCK_STREAM IPPROTO_TCP SO_KEEPALIVE].freeze

  # What CRuby 3.4.7 on Linux names each of NAMES.
  VALUES = [64, 0, 0, 2, 10, 0, 2_130_706_433, 4_294_967_295, 4_294_967_295, 1024, -3, -1, -4, -6, -10, -2, -8,
            -7, -11, -9, -12, 4, 8, 16, 1025, 32, 2, 1, 6, 9].freeze

  CONSTANTS = <<~RUBY
    names = %<names>s
    [names.map { |name| Socket.const_get(name) }, names.map { |name| Socket::Constants.const_get(name) },
     Socket.ancestors.include?(Socket::Constants)]
  RUBY

  # @behavior NW-224
  def test_a_script_names_each_socket_constant_as_crubys
    assert_equal [VALUES, VALUES, false], Kobako::Sandbox.new.eval(format(CONSTANTS, names: NAMES.inspect)).value,
                 "each Socket constant a script names must hold the number CRuby's on Linux does, on Socket " \
                 "and on Socket::Constants alike, which Socket does not include"
  end

  WAITS = <<~RUBY
    answer = lambda do |&call|
      value = call.call
      value.is_a?(IO) ? :self : value
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    [answer.call { socket.wait(0.01) }, answer.call { socket.wait(1, :write) },
     answer.call { socket.wait(1, :read, :w) }, answer.call { socket.wait(0.01, :bogus) },
     answer.call { socket.wait(0.01, "read") }, answer.call { socket.wait(nil) },
     answer.call { socket.wait(:w, 1) }, answer.call { socket.wait(4, 1) }, answer.call { socket.wait(1, 0.01) },
     answer.call { socket.wait(1, :read, 2) },
     IPSocket.getaddress("127.1"), IPSocket.getaddress("::1"), answer.call { IPSocket.getaddress("unnamed.test") }]
  RUBY

  # What CRuby 3.4.7 on Linux answers for each of WAITS.
  WAIT_ANSWERS = [nil, :self, :self, ["ArgumentError", "unsupported mode: bogus"],
                  ["ArgumentError", "Events must be positive integer!"],
                  ["TypeError", "can't convert NilClass into time interval"], :self, 4, nil,
                  ["ArgumentError", "timeout given more than once"], "127.0.0.1", "::1",
                  ["Socket::ResolutionError", "getaddrinfo: Name or service not known"]].freeze

  # @behavior NW-224
  def test_a_script_waits_and_looks_an_address_up_as_crubys
    with_listening_server do |port|
      assert_equal WAIT_ANSWERS, loopback_sandbox(port).eval(format(WAITS, port: port)).value,
                   "IO#wait must answer nil at its timeout and the IO once ready for any mode named in any " \
                   "place, the ready events for (events, timeout), raise ArgumentError for an unknown mode, " \
                   "events no positive Integer, or a second timeout, and TypeError for a nil timeout, and " \
                   "IPSocket.getaddress must answer the address a host spells, as CRuby's on Linux do"
    end
  end
end
