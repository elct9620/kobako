# frozen_string_literal: true

require "test_helper"

# E2E — a guest socket the host has no thread to watch, through the real
# guest binary. The process's reactor starts with the first socket it
# watches, so the child connects nothing before it can start no thread.
class TestE2ENetworkExhaustedHost < Minitest::Test
  include ExhaustedChild

  CONNECT = <<~RUBY
    CONNECT = 'begin; UDPSocket.new.connect("127.0.0.1", 9); :connected; rescue SystemCallError => e; e.class.to_s; end'
    sandbox = Kobako::Sandbox.new(wasm_path: WASM).network(:local) do |net|
      net.endpoint "127.0.0.1", port: 9, protocol: :udp, addresses: [:local]
    end
    outcome(sandbox, "0")
    refused, failed = without_threads { outcome(sandbox, CONNECT) }
    print [refused, failed, outcome(sandbox, CONNECT)].inspect
  RUBY

  # @behavior NW-249
  def test_a_connect_the_host_has_no_thread_to_watch_answers_eagain
    assert_equal '[true, "Errno::EAGAIN", :connected]', run_exhausted_child(CONNECT),
                 "UDPSocket#connect in a guest whose process's RLIMIT_NPROC refuses Thread.new, before any socket " \
                 "was watched, must raise Errno::EAGAIN in the guest, and a connect once threads are free must succeed"
  end
end
