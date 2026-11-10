# frozen_string_literal: true

require "test_helper"

# E2E — what a script's Addrinfo shows and converts to, through the real Guest
# Binary, as CRuby's does on Linux: each kind and family inspect names, the
# name and service it was looked up by, and the Addrinfo it converts to.
class TestE2ENetworkAddrinfoInspect < Minitest::Test
  include E2eGuestHelper

  INSPECTIONS = <<~RUBY
    refusal = lambda do |&call|
      call.call
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
    packed = Socket.sockaddr_in(80, "1.2.3.4")
    [Addrinfo.tcp("::ffff:1.2.3.4", 80).ipv6_to_ipv4.inspect,
     Addrinfo.ip("127.0.0.1").family_addrinfo("127.0.0.2", 80).inspect,
     refusal.call { Addrinfo.ip("127.0.0.1").family_addrinfo },
     Addrinfo.new(packed, nil, :STREAM).inspect, Addrinfo.new(packed, nil, 5).inspect,
     Addrinfo.new(packed, :INET, 9, 7).inspect, Addrinfo.tcp("127.0.0.1", "0080").inspect,
     Addrinfo.tcp("", 80).inspect, Addrinfo.tcp("127.0.0.1", "80").inspect, Addrinfo.tcp("127.0.0.1", "").inspect,
     Addrinfo.tcp("127.0.0.1", 65_616).inspect,
     Addrinfo.new(packed, :INET, :RAW, 1).inspect,
     refusal.call { Addrinfo.ip("127.0.0.1").family_addrinfo("127.0.0.2") }]
  RUBY

  ANSWERS = ["#<Addrinfo: 1.2.3.4 TCP>", "#<Addrinfo: 127.0.0.2:80 TCP>", ["ArgumentError", "no address specified"],
             "#<Addrinfo: 1.2.3.4:80 SOCK_STREAM>", "#<Addrinfo: 1.2.3.4:80 SOCK_SEQPACKET>",
             "#<Addrinfo: 1.2.3.4:80 SOCK_???(9) UNKNOWN_PROTOCOL(7)>", "#<Addrinfo: 127.0.0.1:80 TCP (:0080)>",
             "#<Addrinfo: 0.0.0.0:80 TCP ()>", "#<Addrinfo: 127.0.0.1:80 TCP>", "#<Addrinfo: 127.0.0.1 TCP (:)>",
             "#<Addrinfo: 127.0.0.1:80 TCP (:65616)>",
             "#<Addrinfo: 1.2.3.4:80 SOCK_RAW IPPROTO_ICMP>",
             ["ArgumentError", "IP address needs host and port but 1 arguments given"]].freeze

  # @behavior NW-140
  def test_an_addrinfo_shows_and_converts_as_crubys
    assert_equal ANSWERS, Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(INSPECTIONS).value,
                 "Addrinfo#inspect must name a kind only for an IP family, unknown kinds by number, each IP " \
                 "protocol by name, and a service spelled apart from its port; #ipv6_to_ipv4 must drop the port; " \
                 "#family_addrinfo must keep the lookup's kind and refuse anything but a host and a port"
  end
end
