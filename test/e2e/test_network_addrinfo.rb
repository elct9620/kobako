# frozen_string_literal: true

require "test_helper"

# E2E — a script's Addrinfo through the real Guest Binary: what one built from
# a literal, a packed address, an array, or a name answers, as CRuby's does on
# Linux, with a name standing for its virtual address.
class TestE2ENetworkAddrinfo < Minitest::Test
  include E2eGuestHelper

  BUILT = <<~RUBY
    infos = [Addrinfo.tcp("127.0.0.1", 80), Addrinfo.udp("10.0.0.1", "53"), Addrinfo.ip("::1"),
             Addrinfo.tcp("::1", 443), Addrinfo.new(Socket.sockaddr_in(80, "1.2.3.4")),
             Addrinfo.new(["AF_INET", 80, "x", "1.2.3.4"], nil, :DGRAM), Addrinfo.tcp("", 80)]
    parts = infos.map do |a|
      [a.inspect, a.afamily, a.pfamily, a.socktype, a.protocol, a.ipv4?, a.ipv6?, a.unix?, a.ip_unpack,
       a.inspect_sockaddr, a.to_sockaddr == Socket.sockaddr_in(a.ip_port, a.ip_address)]
    end
    tests = %i[ipv4_loopback? ipv4_private? ipv4_multicast? ipv6_loopback? ipv6_unspecified? ipv6_linklocal?
               ipv6_sitelocal? ipv6_unique_local? ipv6_multicast? ipv6_v4mapped? ipv6_v4compat? ipv6_mc_linklocal?
               ipv6_mc_global?]
    held = %w[127.1.2.3 172.16.0.1 224.0.0.1 :: fe80::1 fec0::1 ff02::1 ::ffff:1.2.3.4 ::1.2.3.4 fc00::1 ff0e::1]
    predicates = held.map { |host| tests.select { |test| Addrinfo.ip(host).public_send(test) } }
    named = Addrinfo.tcp("localhost", 80)
    refused = begin
      Addrinfo.new("xx")
    rescue ArgumentError => e
      e.message
    end
    [parts, predicates, Addrinfo.ip("::ffff:1.2.3.4").ipv6_to_ipv4.inspect, Addrinfo.ip("::1").ipv6_to_ipv4,
     named.inspect, named.getnameinfo, named.family_addrinfo("1.1.1.1", 99).inspect, refused,
     %i[connect bind listen].map { |name| named.respond_to?(name) }]
  RUBY

  PARTS = [
    ["#<Addrinfo: 127.0.0.1:80 TCP>", 2, 2, 1, 6, true, false, false, ["127.0.0.1", 80], "127.0.0.1:80", true],
    ["#<Addrinfo: 10.0.0.1:53 UDP>", 2, 2, 2, 17, true, false, false, ["10.0.0.1", 53], "10.0.0.1:53", true],
    ["#<Addrinfo: ::1>", 10, 10, 0, 0, false, true, false, ["::1", 0], "::1", true],
    ["#<Addrinfo: [::1]:443 TCP>", 10, 10, 1, 6, false, true, false, ["::1", 443], "[::1]:443", true],
    ["#<Addrinfo: 1.2.3.4:80>", 2, 0, 0, 0, true, false, false, ["1.2.3.4", 80], "1.2.3.4:80", true],
    ["#<Addrinfo: 1.2.3.4:80 UDP (x)>", 2, 2, 2, 0, true, false, false, ["1.2.3.4", 80], "1.2.3.4:80", true],
    ["#<Addrinfo: 0.0.0.0:80 TCP ()>", 2, 2, 1, 6, true, false, false, ["0.0.0.0", 80], "0.0.0.0:80", true]
  ].freeze

  PREDICATES = [
    %i[ipv4_loopback?], %i[ipv4_private?], %i[ipv4_multicast?], %i[ipv6_unspecified?], %i[ipv6_linklocal?],
    %i[ipv6_sitelocal?], %i[ipv6_multicast? ipv6_mc_linklocal?], %i[ipv6_v4mapped?], %i[ipv6_v4compat?],
    %i[ipv6_unique_local?], %i[ipv6_multicast? ipv6_mc_global?]
  ].freeze

  NAMED = ["#<Addrinfo: 1.2.3.4>", nil, "#<Addrinfo: 198.18.0.0:80 TCP (localhost)>", ["198.18.0.0", "80"],
           "#<Addrinfo: 1.1.1.1:99 TCP>", "not an AF_INET/AF_INET6 sockaddr", [false, false, false]].freeze

  # @behavior NW-140
  def test_a_script_builds_and_reads_an_addrinfo_as_crubys
    assert_equal [PARTS, PREDICATES, *NAMED], naming_localhost.eval(BUILT).value,
                 "Addrinfo.tcp, .udp, .ip, and .new must answer each part and #inspect as CRuby's on Linux, a name " \
                 "its virtual address, #getnameinfo the address, a packed address of no IP family ArgumentError, " \
                 "and no #connect, #bind, or #listen"
  end

  LOOKED_UP = <<~RUBY
    refusal = lambda do |&call|
      call.call
    rescue SocketError => e
      [e.class.to_s, e.message]
    end
    [Addrinfo.getaddrinfo("127.0.0.1", 80).map(&:inspect), Addrinfo.getaddrinfo("127.0.0.1", 80, :INET, :STREAM).map(&:inspect),
     Addrinfo.getaddrinfo("10.0.0.1", "53", nil, nil, Socket::IPPROTO_UDP).map(&:inspect),
     Addrinfo.getaddrinfo(nil, 80, :INET, :STREAM, nil, Socket::AI_PASSIVE).map(&:inspect),
     Addrinfo.getaddrinfo("localhost", 80, nil, :STREAM, nil, Socket::AI_CANONNAME).map { |a| [a.inspect, a.canonname] },
     Socket.getaddrinfo("localhost", "80", :INET, :STREAM),
     refusal.call { Addrinfo.getaddrinfo("127.0.0.1", 80, :INET6) }, refusal.call { Addrinfo.getaddrinfo("unnamed.test", 80) }]
  RUBY

  LOOKUPS = [
    ["#<Addrinfo: 127.0.0.1:80 TCP>", "#<Addrinfo: 127.0.0.1:80 UDP>", "#<Addrinfo: 127.0.0.1:80 SOCK_RAW>"],
    ["#<Addrinfo: 127.0.0.1:80 TCP>"],
    ["#<Addrinfo: 10.0.0.1:53 UDP>", "#<Addrinfo: 10.0.0.1:53 SOCK_RAW IPPROTO_UDP>"],
    ["#<Addrinfo: 0.0.0.0:80 TCP>"],
    [["#<Addrinfo: 198.18.0.0:80 TCP localhost (localhost)>", "localhost"]],
    [["AF_INET", 80, "198.18.0.1", "198.18.0.1", 2, 1, 6]],
    ["Socket::ResolutionError", "getaddrinfo: Address family for hostname not supported"],
    ["Socket::ResolutionError", "getaddrinfo: Name or service not known"]
  ].freeze

  # @behavior NW-141
  def test_a_script_looks_up_a_names_addrinfos_as_crubys
    assert_equal LOOKUPS, naming_localhost.eval(LOOKED_UP).value,
                 "Addrinfo.getaddrinfo and Socket.getaddrinfo must answer one entry per kind CRuby's does, a name's " \
                 "virtual address, the canonical name on the first entry, and Socket::ResolutionError for a family " \
                 "the address lacks and a name no entry names"
  end

  def naming_localhost
    Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:local) do |net|
      net.endpoint "localhost", port: 80, protocol: :tcp, addresses: [:local]
    end
  end
end
