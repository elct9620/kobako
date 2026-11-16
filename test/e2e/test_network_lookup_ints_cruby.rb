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
end
