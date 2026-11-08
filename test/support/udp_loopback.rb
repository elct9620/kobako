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

  # A Sandbox whose one UDP endpoint reaches loopback +port+.
  def sandbox_sending_to(port)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM).network(:local) do |net|
      net.endpoint "127.0.0.1", port: port, protocol: :udp, addresses: [:local]
    end
  end
end
