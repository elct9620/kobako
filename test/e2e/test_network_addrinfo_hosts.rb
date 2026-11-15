# frozen_string_literal: true

require "test_helper"

# E2E — the hosts a script's lookup reads as literals without a resolver,
# through the real Guest Binary: an empty host and the two names CRuby
# brackets.
class TestE2ENetworkAddrinfoHosts < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  SPELLED_LITERALS = <<~RUBY
    ["", "<any>", "<broadcast>"].map do |host|
      [Socket::AI_NUMERICHOST, Socket::AI_CANONNAME].map do |flag|
        Addrinfo.getaddrinfo(host, 80, :INET, :STREAM, nil, flag).map { |a| [a.ip_address, a.canonname] }
      end
    end
  RUBY

  # @behavior NW-141
  def test_a_lookup_reads_an_empty_host_and_crubys_bracketed_names_as_literals
    any, broadcast = %w[0.0.0.0 255.255.255.255].map { |ip| [[[ip, nil]]] * 2 }

    assert_equal [any, any, broadcast], localhost_sandbox(80).eval(SPELLED_LITERALS).value,
                 'Addrinfo.getaddrinfo of "", "<any>", or "<broadcast>" must answer the address CRuby reads each ' \
                 "as, under AI_NUMERICHOST too, and carry no canonical name under AI_CANONNAME"
  end
end
