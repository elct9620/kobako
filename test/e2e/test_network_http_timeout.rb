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

  NEGATIVE_READ = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    http.read_timeout = -1
    begin
      http.get("/")
    rescue ArgumentError => e
      [e.class.to_s, e.message]
    end
  RUBY

  # @behavior NW-177
  def test_a_timeout_crubys_would_refuse_raises_before_the_request_leaves
    server = TCPServer.new("127.0.0.1", 0)
    port = server.addr[1]
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:web) { |net| net.endpoint "127.0.0.1", port: port }
    value = sandbox.eval(format(NEGATIVE_READ, port: port)).value
    reached = server.wait_readable(0.2) ? :reached : :untouched

    assert_equal [["ArgumentError", "time interval must not be negative"], :untouched], [value, reached],
                 "Net::HTTP#get with a negative read_timeout must raise ArgumentError before the request reaches " \
                 "the server, where CRuby's raises only once a read waits"
  ensure
    server&.close
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
