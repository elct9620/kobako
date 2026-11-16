# frozen_string_literal: true

require "test_helper"

# E2E — what a script hands a lookup, read as CRuby's on Linux reads it,
# through the real Guest Binary with no lookup made: a host's Integer, NUL,
# and length, a short IPv4 form, a call naming nothing, an Addrinfo's array,
# and the flags getaddrinfo and getnameinfo take.
class TestE2ENetworkLookupArgumentsCruby < Minitest::Test
  include E2eGuestHelper

  # Each call's answer, or the class and message it raises, and the
  # getaddrinfo code a Socket::ResolutionError carries.
  PROBE = <<~RUBY
    answer = lambda do |&call|
      shown = ->(value) { value.is_a?(Addrinfo) ? value.inspect : value }
      value = call.call
      value.is_a?(Array) ? value.map(&shown) : shown.call(value)
    rescue StandardError => e
      [e.class.to_s, e.respond_to?(:error_code) ? e.error_code : nil, e.message]
    end
  RUBY

  def answers_of(calls)
    script = PROBE + "[#{calls.join(", ")}].map { |call| answer.call(&call) }"
    Kobako::Sandbox.new.eval(script).value
  end

  def resolution(code, words) = ["Socket::ResolutionError", code, "getaddrinfo: #{words}"]

  HOSTS = [
    "-> { Addrinfo.tcp(4_294_967_295, 80) }", "-> { Addrinfo.tcp(-1, 80) }",
    "-> { Addrinfo.tcp(4_294_967_296, 80) }", "-> { Addrinfo.tcp(-2_147_483_649, 80) }",
    '-> { Addrinfo.tcp("a\\0b", 80) }', '-> { Addrinfo.tcp("127.0.0.1", "80\\0") }',
    '-> { Addrinfo.tcp("x" * 1025, 80) }'
  ].freeze

  # @behavior NW-190
  def test_a_host_reads_as_crubys_host_str_bounds_it
    assert_equal ["#<Addrinfo: 255.255.255.255:80 TCP>", "#<Addrinfo: 255.255.255.255:80 TCP>",
                  ["RangeError", nil, "integer 4294967296 too big to convert to 'unsigned int'"],
                  ["RangeError", nil, "integer -2147483649 too small to convert to 'unsigned int'"],
                  ["ArgumentError", nil, "string contains null byte"],
                  ["ArgumentError", nil, "string contains null byte"],
                  ["ArgumentError", nil, "hostname too long (1025)"]], answers_of(HOSTS),
                 "Addrinfo.tcp must read an Integer host as NUM2UINT does, raising RangeError past an unsigned " \
                 "int, and a host or port holding NUL or a host of 1025 bytes must raise ArgumentError, as " \
                 "CRuby's host_str and port_str do"
  end

  SHORT_FORMS = %w[127.1 0x7f.1 0177.1 2130706433 127.256 1.2.3 01.02.03.04].map do |host|
    "-> { Addrinfo.tcp(#{host.inspect}, 80) }"
  end.freeze

  # @behavior NW-201
  def test_a_short_ipv4_form_is_its_address_with_no_lookup
    assert_equal ["#<Addrinfo: 127.0.0.1:80 TCP (127.1)>", "#<Addrinfo: 127.0.0.1:80 TCP (0x7f.1)>",
                  "#<Addrinfo: 127.0.0.1:80 TCP (0177.1)>", "#<Addrinfo: 127.0.0.1:80 TCP (2130706433)>",
                  "#<Addrinfo: 127.0.1.0:80 TCP (127.256)>", "#<Addrinfo: 1.2.0.3:80 TCP (1.2.3)>",
                  "#<Addrinfo: 1.2.3.4:80 TCP (01.02.03.04)>"], answers_of(SHORT_FORMS),
                 "Addrinfo.tcp in a Sandbox with no network must read each short IPv4 form as inet_aton does, " \
                 "with no lookup, as CRuby's getaddrinfo on Linux answers it"
  end

  LOOKUPS = [
    "-> { Addrinfo.getaddrinfo(nil, nil) }", "-> { Addrinfo.tcp(nil, nil) }",
    "-> { Socket.sockaddr_in(nil, nil) }",
    "-> { Addrinfo.getaddrinfo(nil, 80, nil, :STREAM, nil, Socket::AI_CANONNAME) }",
    "-> { Addrinfo.getaddrinfo('127.1', 80, nil, :STREAM, nil, Socket::AI_NUMERICHOST) }",
    "-> { Addrinfo.tcp('127.0.0.1', 80).getnameinfo(nil) }",
    "-> { Addrinfo.tcp('127.0.0.1', 80).getnameinfo(-1) }"
  ].freeze

  # @behavior NW-141
  def test_a_lookup_refuses_what_glibcs_getaddrinfo_refuses_first
    nameless = resolution(-2, "Name or service not known")
    assert_equal [nameless, nameless, nameless, resolution(-1, "Bad value for ai_flags"),
                  ["#<Addrinfo: 127.0.0.1:80 TCP (127.1)>"], %w[127.0.0.1 80],
                  ["Socket::ResolutionError", -1, "getnameinfo: Bad value for ai_flags"]], answers_of(LOOKUPS),
                 "a lookup naming neither host nor service must raise EAI_NONAME, AI_CANONNAME with no host " \
                 "EAI_BADFLAGS, and AI_NUMERICHOST must take a short IPv4 form; getnameinfo must take nil as no " \
                 "flags and raise EAI_BADFLAGS for a flag glibc's lacks, as CRuby's on Linux do"
  end

  ARRAYS = [
    '["AF_INET6", 80, nil, "1.2.3.4"]', '["FOO", 80, nil, "1.2.3.4"]', '[2, 80, nil, "1.2.3.4"]',
    '["AF_INET", 80, :h, "1.2.3.4"]', '["AF_INET", 70000, nil, "1.2.3.4"]', '["AF_INET", 80, nil, "127.1"]',
    '["AF_INET", 80, nil, nil]', '["PF_INET", 80, nil, "1.2.3.4"]',
    '["AF_INET", 80, nil, "1.2.3.4"], :INET6', '["AF_INET", 80, nil, "1.2.3.4"], nil, :STREAM, 17'
  ].map { |parts| "-> { Addrinfo.new(#{parts}) }" }.freeze

  UNMATCHED = ["Socket::ResolutionError", -9, "getaddrinfo: Address family for hostname not supported"].freeze

  ARRAY_ANSWERS = [
    UNMATCHED, ["SocketError", nil, "unknown address family: FOO"],
    ["TypeError", nil, "no implicit conversion of Integer into String"],
    ["TypeError", nil, "no implicit conversion of Symbol into String"],
    "#<Addrinfo: 1.2.3.4:4464 (:70000)>", "#<Addrinfo: 127.0.0.1:80>",
    ["TypeError", nil, "no implicit conversion of nil into String"], "#<Addrinfo: 1.2.3.4:80>",
    UNMATCHED, ["Socket::ResolutionError", -7, "getaddrinfo: ai_socktype not supported"]
  ].freeze

  # @behavior NW-140
  def test_an_addrinfo_reads_its_array_as_crubys
    assert_equal ARRAY_ANSWERS, answers_of(ARRAYS),
                 "Addrinfo.new must read an array as CRuby's numeric lookup of it: a family it does not hold, " \
                 "by its first element or by family, raises EAI_ADDRFAMILY, an unnamed family SocketError, a " \
                 "part of the wrong class TypeError, a port its low 16 bits, and a kind glibc's table lacks " \
                 "EAI_SOCKTYPE"
  end
end
