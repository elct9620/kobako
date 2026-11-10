# frozen_string_literal: true

require "test_helper"

# E2E — a script reading its socket's ends as Addrinfos, through the real
# Guest Binary against loopback peers: of the socket's kind, at the end addr
# and peeraddr answer.
class TestE2ENetworkSocketAddrinfo < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback
  include UdpLoopback

  ENDS = <<~RUBY
    tcp = TCPSocket.new("127.0.0.1", %<tcp>d)
    udp = UDPSocket.new
    unconnected = begin
      udp.remote_address
    rescue SystemCallError => e
      e.class.to_s
    end
    local = udp.local_address.inspect
    udp.connect("127.0.0.1", %<udp>d)
    kinds = [tcp, udp].map { |socket| [socket.remote_address.socktype, socket.remote_address.protocol] }
    [tcp.local_address.inspect, tcp.addr[1], tcp.remote_address.inspect, local, unconnected, udp.remote_address.inspect,
     kinds]
  RUBY

  # @behavior NW-142
  def test_a_script_reads_a_sockets_ends_as_addrinfos
    with_listening_server do |tcp|
      with_quiet_udp_peer do |udp|
        value = tcp_and_udp_sandbox(tcp, udp).eval(format(ENDS, tcp: tcp, udp: udp)).value

        assert_equal ["#<Addrinfo: 198.19.255.254:#{value[1]} TCP>", value[1], "#<Addrinfo: 127.0.0.1:#{tcp} TCP>",
                      "#<Addrinfo: 0.0.0.0 UDP>", "Errno::ENOTCONN", "#<Addrinfo: 127.0.0.1:#{udp} UDP>",
                      [[1, 6], [2, 17]]], value,
                     "#local_address and #remote_address must answer the Addrinfo of the end #addr and #peeraddr " \
                     "answer, of the socket's kind, and a UDPSocket with no peer raise Errno::ENOTCONN"
      end
    end
  end
end
