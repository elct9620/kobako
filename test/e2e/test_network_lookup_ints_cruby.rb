# frozen_string_literal: true

require "test_helper"

# E2E — each number a lookup reads as C's int, as CRuby's NUM2INT bounds it
# on Linux, through the real Guest Binary with no lookup made.
class TestE2ENetworkLookupIntsCruby < Minitest::Test
  include E2eGuestHelper

  BIG = 2**31

  # Each call's answer, or the class and message it raises.
  PROBE = <<~RUBY
    answer = lambda do |&call|
      call.call
      :answered
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
  RUBY

  # getaddrinfo's flags, protocol, kind, and family, an Addrinfo's protocol,
  # and getnameinfo's flags past the type's top, and flags under its bottom.
  CALLS = [
    "-> { Addrinfo.getaddrinfo('127.0.0.1', 80, nil, nil, nil, #{BIG}) }",
    "-> { Addrinfo.getaddrinfo('127.0.0.1', 80, nil, nil, nil, #{-BIG - 1}) }",
    "-> { Addrinfo.getaddrinfo('127.0.0.1', 80, nil, nil, #{BIG}) }",
    "-> { Addrinfo.getaddrinfo('127.0.0.1', 80, nil, #{BIG}) }",
    "-> { Addrinfo.getaddrinfo('127.0.0.1', 80, #{BIG}) }",
    "-> { Addrinfo.new(['AF_INET', 80, '127.0.0.1', '127.0.0.1'], nil, nil, #{BIG}) }",
    "-> { Addrinfo.tcp('127.0.0.1', 80).getnameinfo(#{BIG}) }",
    "-> { Socket.getaddrinfo('127.0.0.1', 80, nil, nil, nil, #{BIG}) }"
  ].freeze

  def too(side, number) = ["RangeError", "integer #{number} too #{side} to convert to 'int'"]

  # @behavior NW-141
  def test_a_lookups_numbers_read_as_crubys_num2int_bounds_them
    script = PROBE + "[#{CALLS.join(", ")}].map { |call| answer.call(&call) }"
    big = too("big", BIG)

    assert_equal [big, too("small", -BIG - 1), big, big, big, big, big, big], Kobako::Sandbox.new.eval(script).value,
                 "a lookup's flags, protocol, kind, or family, an Addrinfo's protocol, and getnameinfo's flags " \
                 "past C's int must raise RangeError too big or too small, as CRuby's NUM2INT does on Linux"
  end

  # A family, kind, or protocol CRuby refuses beside an int past its range,
  # each read in CRuby's call_getaddrinfo and addrinfo_initialize order.
  ORDER_CALLS = [
    "-> { Addrinfo.getaddrinfo('127.0.0.1', 80, :NOPE, nil, nil, #{BIG}) }",
    "-> { Addrinfo.getaddrinfo('127.0.0.1', 80, nil, :NOPE, #{BIG}) }",
    "-> { Addrinfo.getaddrinfo('127.0.0.1', 80, nil, nil, #{BIG}, 'x') }",
    "-> { Socket.getaddrinfo('127.0.0.1', 80, :NOPE, nil, nil, #{BIG}) }",
    "-> { Addrinfo.new(['AF_INET', 80, nil, '127.0.0.1'], nil, :NOPE, #{BIG}) }",
    "-> { Addrinfo.new(['AF_BOGUS', 80, nil, '127.0.0.1'], nil, :NOPE) }",
    "-> { Addrinfo.new(['AF_INET', 80, nil, '127.0.0.1'], :NOPE, nil, #{BIG}) }"
  ].freeze

  # @behavior NW-141
  def test_a_lookup_reads_its_family_kind_protocol_and_flags_in_crubys_order
    domain = ["SocketError", "unknown socket domain: NOPE"]
    type = ["SocketError", "unknown socket type: NOPE"]
    script = PROBE + "[#{ORDER_CALLS.join(", ")}].map { |call| answer.call(&call) }"

    assert_equal [domain, type, too("big", BIG), domain, type, type, domain], Kobako::Sandbox.new.eval(script).value,
                 "a lookup or an Addrinfo.new naming an unknown family or kind beside an int past C's range must " \
                 "raise SocketError for the family, then the kind, before the protocol and the flags raise " \
                 "RangeError, and the arguments before the array's own family, as CRuby's do on Linux"
  end
end
