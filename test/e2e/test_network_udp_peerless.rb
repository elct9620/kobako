# frozen_string_literal: true

require "test_helper"

# E2E — a script reading a UDPSocket no connect named a peer for, through the
# real Guest Binary: every read, waiting or not, is refused, since nothing may
# arrive on a socket the guest streams with no peer.
class TestE2ENetworkUdpPeerless < Minitest::Test
  include E2eGuestHelper
  include UdpLoopback

  READS = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      e.class.to_s
    end
    socket = UDPSocket.new
    [
      outcome.call { socket.recvfrom(1) }, outcome.call { socket.recv(1) }, outcome.call { socket.read(1) },
      outcome.call { socket.recvfrom_nonblock(1) }, outcome.call { socket.recv_nonblock(1) },
      outcome.call { socket.recvfrom_nonblock(1, exception: false) }
    ]
  RUBY

  # @behavior NW-181
  def test_a_read_with_no_peer_raises_eacces_waiting_or_not
    value = udp_sandbox(9).eval(READS).value

    assert_equal ["Errno::EACCES"] * 6, value,
                 "UDPSocket#recvfrom, #recv, #read, #recvfrom_nonblock, #recv_nonblock, and #recvfrom_nonblock " \
                 "with exception: false on a socket no #connect named a peer for must each raise Errno::EACCES, " \
                 "where CRuby's would wait or answer EAGAIN"
  end
end
