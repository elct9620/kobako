# frozen_string_literal: true

require "test_helper"

# E2E — a script's socket addresses, through the real Guest Binary: what
# Socket.sockaddr_in packs and unpacks as Linux lays a sockaddr out, and the
# ends a socket reads back, packed and as arrays.
class TestE2ENetworkSocketEnds < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback
  include UdpLoopback

  PACKS = <<~RUBY
    pack = ->(port, host) { Socket.sockaddr_in(port, host).bytes }
    packed = [
      pack.call(80, "127.0.0.1"), pack.call("80", "10.0.0.1").size, pack.call(0, ""), pack.call(1, "<broadcast>"),
      pack.call(1, "<any>"), pack.call(1, nil), pack.call(1, 5), pack.call(443, "::1"), pack.call(70_000, "1.2.3.4")
    ]
    unpack = ->(port, host) { Socket.unpack_sockaddr_in(Socket.pack_sockaddr_in(port, host)) }
    refused = begin
      Socket.unpack_sockaddr_in("xx")
    rescue ArgumentError => e
      e.message
    end
    [packed, unpack.call(8080, "1.2.3.4"), unpack.call(1, "fe80::1"), unpack.call(9, "localhost"), refused]
  RUBY

  PACKED = [
    [2, 0, 0, 80, 127, 0, 0, 1] + ([0] * 8),
    16,
    [2, 0, 0, 0, 0, 0, 0, 0] + ([0] * 8),
    [2, 0, 0, 1, 255, 255, 255, 255] + ([0] * 8),
    [2, 0, 0, 1, 0, 0, 0, 0] + ([0] * 8),
    [2, 0, 0, 1, 127, 0, 0, 1] + ([0] * 8),
    [2, 0, 0, 1, 0, 0, 0, 5] + ([0] * 8),
    [10, 0, 1, 187] + ([0] * 4) + ([0] * 15) + [1] + ([0] * 4),
    [2, 0, 17, 112, 1, 2, 3, 4] + ([0] * 8)
  ].freeze

  # @behavior NW-138
  def test_a_script_packs_and_unpacks_a_socket_address_as_crubys
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:local) do |net|
      net.endpoint "localhost", port: 9, protocol: :tcp, addresses: [:local]
    end

    assert_equal [PACKED, [8080, "1.2.3.4"], [1, "fe80::1"], [9, "198.18.0.0"], "not an AF_INET/AF_INET6 sockaddr"],
                 sandbox.eval(PACKS).value,
                 "Socket.sockaddr_in must pack each host and port as Linux's sockaddr_in or sockaddr_in6, nil as " \
                 "127.0.0.1 and a name as its virtual address; Socket.unpack_sockaddr_in must answer [port, ip] " \
                 "and raise ArgumentError for a String no family lays out"
  end

  ENDS = <<~RUBY
    unpack = ->(sockaddr) { Socket.unpack_sockaddr_in(sockaddr) }
    outcome = lambda do |&call|
      call.call
    rescue IOError, SystemCallError => e
      [e.class.to_s, e.message]
    end
    tcp = TCPSocket.new("127.0.0.1", %<tcp>d)
    tcp_ends = [unpack.call(tcp.getsockname), tcp.addr, unpack.call(tcp.getpeername), tcp.peeraddr]
    tcp.close
    closed = [outcome.call { tcp.getsockname }, outcome.call { tcp.peeraddr }]
    udp = UDPSocket.new
    unconnected = [unpack.call(udp.getsockname), outcome.call { udp.getpeername }, outcome.call { udp.peeraddr }]
    udp.connect("127.0.0.1", %<udp>d)
    [tcp_ends, closed, unconnected, [unpack.call(udp.getpeername), udp.peeraddr(:numeric)]]
  RUBY

  # @behavior NW-139
  def test_a_script_reads_a_sockets_ends_as_crubys
    with_listening_server do |tcp|
      with_quiet_udp_peer do |udp|
        value = sandbox_reaching_both(tcp, udp).eval(format(ENDS, tcp: tcp, udp: udp)).value

        assert_equal ends_answered(tcp, udp, value.dig(0, 1, 1)), value,
                     "#getsockname and #getpeername must unpack to what #addr and #peeraddr answer, the virtual " \
                     "own end included; a UDPSocket with no peer must raise Errno::ENOTCONN for its peer, and a " \
                     "closed socket IOError"
      end
    end
  end

  # What ENDS answers for the loopback ports, its own end on the virtual
  # address at +local_port+.
  def ends_answered(tcp, udp, local_port)
    own = "198.19.255.254"
    unconnected = ["Errno::ENOTCONN", "Transport endpoint is not connected - getpeername(2)"]
    [
      [[local_port, own], ["AF_INET", local_port, own, own], [tcp, "127.0.0.1"],
       ["AF_INET", tcp, "127.0.0.1", "127.0.0.1"]],
      [["IOError", "closed stream"]] * 2,
      [[0, "0.0.0.0"], unconnected, unconnected],
      [[udp, "127.0.0.1"], ["AF_INET", udp, "127.0.0.1", "127.0.0.1"]]
    ]
  end
end
