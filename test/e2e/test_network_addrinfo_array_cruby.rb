# frozen_string_literal: true

require "test_helper"

# E2E — an Addrinfo a script builds from CRuby's ["AF_INET", port, host, ip]
# array, read as CRuby's numeric lookup of it on Linux, through the real
# Guest Binary with no lookup made.
class TestE2ENetworkAddrinfoArrayCruby < Minitest::Test
  include E2eGuestHelper

  # Each call's answer, or the class and message it raises, and the
  # getaddrinfo code a Socket::ResolutionError carries.
  PROBE = <<~RUBY
    answer = lambda do |&call|
      value = call.call
      value.is_a?(Addrinfo) ? value.inspect : value
    rescue StandardError => e
      [e.class.to_s, e.respond_to?(:error_code) ? e.error_code : nil, e.message]
    end
  RUBY

  def answers_of(calls)
    script = PROBE + "[#{calls.join(", ")}].map { |call| answer.call(&call) }"
    Kobako::Sandbox.new.eval(script).value
  end

  ARRAYS = [
    '["AF_INET6", 80, nil, "1.2.3.4"]', '["FOO", 80, nil, "1.2.3.4"]', '[2, 80, nil, "1.2.3.4"]',
    '["AF_INET", 80, :h, "1.2.3.4"]', '["AF_INET", 70000, nil, "1.2.3.4"]', '["AF_INET", 80, nil, "127.1"]',
    '["AF_INET", 80, nil, nil]', '["PF_INET", 80, nil, "1.2.3.4"]',
    '["AF_INET", 80, nil, "1.2.3.4"], :INET6', '["AF_INET", 80, nil, "1.2.3.4"], nil, :STREAM, 17',
    '["AF_INET", -1, nil, "127.0.0.1"]', '["AF_INET", 2**31, nil, "1.2.3.4"]',
    '["AF_INET", 80, nil, "x"], nil, :DGRAM, 6', '["AF_PF_INET", 80, nil, "1.2.3.4"]',
    '["AF_INET", 80, nil, "::1"], :INET6'
  ].map { |parts| "-> { Addrinfo.new(#{parts}) }" }.freeze

  UNMATCHED = ["Socket::ResolutionError", -9, "getaddrinfo: Address family for hostname not supported"].freeze

  ARRAY_ANSWERS = [
    UNMATCHED, ["SocketError", nil, "unknown address family: FOO"],
    ["TypeError", nil, "no implicit conversion of Integer into String"],
    ["TypeError", nil, "no implicit conversion of Symbol into String"],
    "#<Addrinfo: 1.2.3.4:4464 (:70000)>", "#<Addrinfo: 127.0.0.1:80>",
    ["TypeError", nil, "no implicit conversion of nil into String"], "#<Addrinfo: 1.2.3.4:80>",
    UNMATCHED, ["Socket::ResolutionError", -7, "getaddrinfo: ai_socktype not supported"],
    ["Socket::ResolutionError", -8, "getaddrinfo: Servname not supported for ai_socktype"],
    ["RangeError", nil, "integer 2147483648 too big to convert to 'int'"],
    ["Socket::ResolutionError", -7, "getaddrinfo: ai_socktype not supported"],
    ["SocketError", nil, "unknown address family: AF_PF_INET"], "#<Addrinfo: [::1]:80>"
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
