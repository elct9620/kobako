# frozen_string_literal: true

require "socket"

# A loopback UDP peer for the network journeys under test/e2e/, and a Sandbox
# whose one UDP endpoint reaches it.
module UdpLoopback
  # Yield the port of a loopback peer that answers a datagram upcased.
  def with_upcase_peer
    peer = UDPSocket.new
    peer.bind("127.0.0.1", 0)
    thread = Thread.new do
      mesg, from = peer.recvfrom(16)
      peer.send(mesg.upcase, 0, from[3], from[1])
    end
    yield peer.addr[1]
  ensure
    thread&.kill
    peer&.close
  end

  # Yield the port of a loopback peer answering each datagram with "hello".
  def with_hello_peer
    peer = UDPSocket.new
    peer.bind("127.0.0.1", 0)
    thread = Thread.new { loop { peer.send("hello", 0, *peer.recvfrom(16).last.values_at(3, 1)) } }
    yield peer.addr[1]
  ensure
    thread&.kill
    peer&.close
  end

  # Yield the port of a loopback peer that answers nothing.
  def with_quiet_udp_peer
    peer = UDPSocket.new
    peer.bind("127.0.0.1", 0)
    yield peer.addr[1]
  ensure
    peer&.close
  end

  # A Sandbox whose one UDP endpoint reaches loopback +port+.
  def udp_sandbox(port)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM).network(:local) do |net|
      net.endpoint "127.0.0.1", port: port, protocol: :udp, addresses: [:local]
    end
  end

  # A Sandbox whose TCP endpoint reaches loopback +tcp+ and UDP endpoint +udp+.
  def tcp_and_udp_sandbox(tcp, udp)
    loopback_sandbox(tcp).network(:peer) do |net|
      net.endpoint "127.0.0.1", port: udp, protocol: :udp, addresses: [:local]
    end
  end
end
