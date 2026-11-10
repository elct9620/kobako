# frozen_string_literal: true

require "test_helper"

# E2E — a script's Net::HTTP timeouts through the real Guest Binary against a
# loopback server that accepts and never answers.
class TestE2ENetworkHttpTimeout < Minitest::Test
  include E2eGuestHelper

  SILENT_SERVER = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    defaults = [http.open_timeout, http.read_timeout, http.write_timeout]
    http.read_timeout = 0.1
    begin
      http.get("/")
    rescue Net::ReadTimeout => e
      [defaults, e.is_a?(Timeout::Error), e.is_a?(RuntimeError)]
    end
  RUBY

  # @behavior NW-068
  def test_a_request_past_its_read_timeout_raises_net_read_timeout
    with_silent_server do |port|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 10)
                               .network(:web) { |net| net.endpoint "127.0.0.1", port: port }

      assert_equal [[60, 60, 60], true, true], sandbox.eval(format(SILENT_SERVER, port: port)).value,
                   "Net::HTTP#get with read_timeout 0.1 against a server that never answers must raise " \
                   "Net::ReadTimeout, a Timeout::Error and a RuntimeError, and each timeout must start at 60"
    end
  end

  # A loopback server that accepts one connection and holds it unanswered
  # until the test ends.
  def with_silent_server
    server = TCPServer.new("127.0.0.1", 0)
    accepted = Queue.new
    thread = Thread.new { accepted << server.accept }
    yield server.addr[1]
  ensure
    accepted&.pop(timeout: 1)&.close
    thread&.kill
    server&.close
  end
end
