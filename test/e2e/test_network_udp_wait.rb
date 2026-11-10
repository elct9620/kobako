# frozen_string_literal: true

require "test_helper"

# E2E — a script waiting to read a UDPSocket connected to no peer, through the
# real Guest Binary, on each profile.
class TestE2ENetworkUdpWait < Minitest::Test
  include E2eGuestHelper

  WAIT = "IO.select([UDPSocket.new])"

  # @behavior NW-173
  def test_a_wait_to_read_a_udp_socket_with_no_peer_lasts_until_the_deadline
    assert_equal [:timed_out, nil], [outcome_on(:permissive), outcome_on(:hermetic)],
                 "IO.select of a UDPSocket with no peer to read, with no timeout, must last until the run's " \
                 "deadline on the permissive profile, as Linux's select waits, and answer nil at once on the " \
                 "hermetic profile, where a wait on no host-side producer ends"
  end

  # What the wait answers on +profile+ under a short timeout, or +:timed_out+
  # once the run's deadline ends it.
  def outcome_on(profile)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, profile: profile, timeout: 0.3)
    sandbox.network(:local) { |net| net.endpoint "127.0.0.1", port: 9, protocol: :udp, addresses: [:local] }
    sandbox.eval(WAIT).value
  rescue Kobako::TimeoutError
    :timed_out
  end
end
