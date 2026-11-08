# frozen_string_literal: true

require "test_helper"

# E2E — the address a script reads for its own end of a socket, through the
# real Guest Binary against loopback peers: the guest's own virtual address
# beside the port its peer sees.
class TestE2ENetworkOwnEnd < Minitest::Test
  include E2eGuestHelper

  OWN_END = <<~RUBY
    stream = TCPSocket.new("127.0.0.1", %<tcp>d)
    datagrams = UDPSocket.new
    unconnected = datagrams.addr
    datagrams.connect("127.0.0.1", %<udp>d)
    datagrams.send("x", 0)
    itself = begin
      TCPSocket.new(stream.addr[3], %<tcp>d)
    rescue Errno::EACCES
      :refused
    end
    [stream.addr, unconnected, datagrams.addr, itself]
  RUBY

  # @behavior NW-112
  def test_a_socket_reads_its_own_end_at_the_guests_virtual_address
    with_peers do |server, peer|
      tcp, udp = [server, peer].map { |socket| socket.addr[1] }
      value = sandbox_reaching(tcp, udp).eval(format(OWN_END, tcp: tcp, udp: udp)).value
      stream_port, datagram_port = ports_seen_by(server, peer)

      assert_equal [own_end(stream_port), ["AF_INET", 0, "0.0.0.0", "0.0.0.0"], own_end(datagram_port), :refused],
                   value,
                   "TCPSocket#addr and a connected UDPSocket#addr must answer 198.19.255.254 and the port the " \
                   "peer sees, an unconnected UDPSocket#addr 0.0.0.0 port 0, and a connect to that address " \
                   "Errno::EACCES"
    end
  end

  # The +addr+ a socket reads for its own end at +port+.
  def own_end(port)
    ["AF_INET", port, "198.19.255.254", "198.19.255.254"]
  end

  # The ports the script came from, as +server+ and +peer+ saw them.
  def ports_seen_by(server, peer)
    [server.accept.peeraddr[1], peer.recvfrom(1)[1][1]]
  end

  # Yield a loopback TCP server that only listens and a bound UDP peer.
  def with_peers
    server = TCPServer.new("127.0.0.1", 0)
    peer = UDPSocket.new
    peer.bind("127.0.0.1", 0)
    yield server, peer
  ensure
    server&.close
    peer&.close
  end

  def sandbox_reaching(tcp, udp)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:local) do |net|
      net.endpoint "127.0.0.1", port: tcp, protocol: :tcp, addresses: [:local]
      net.endpoint "127.0.0.1", port: udp, protocol: :udp, addresses: [:local]
    end
  end
end
