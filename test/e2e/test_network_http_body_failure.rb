# frozen_string_literal: true

require "test_helper"

# E2E — how a Net::HTTP request body, which streams to the server after the
# request leaves, reaches the script when its write fails: one a TRACE may
# not carry, and one nothing takes.
class TestE2ENetworkHttpBodyFailure < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  # A body past what the host takes at once, so it streams after the request.
  LARGE_TRACE = <<~RUBY
    class Trace < Net::HTTPRequest
      METHOD = "TRACE"
      REQUEST_HAS_BODY = true
    end
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    http.read_timeout = 1
    begin
      http.request(Trace.new("/"), "x" * 100_000)
    rescue SystemCallError, IOError => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-230
  def test_a_request_body_a_trace_may_not_carry_raises_epipe
    with_listening_server do |port|
      assert_equal "Errno::EPIPE", http_sandbox(port).eval(format(LARGE_TRACE, port: port)).value,
                   "Net::HTTP#request of a TRACE with a 100000-byte body through Sandbox#eval must raise " \
                   "Errno::EPIPE once the body's write is refused, as a TRACE carries no content"
    end
  end

  UNTAKEN_POST = <<~RUBY
    http = Net::HTTP.new("held.test", 80)
    http.write_timeout = 0.01
    begin
      http.post("/", "x" * 1_000_000)
    rescue Timeout::Error => e
      e.class.to_s
    end
  RUBY

  # A name the Mock holds unanswered never connects, so nothing takes the
  # body past the buffer the host holds, whatever the host's own buffers.
  # @behavior NW-238
  def test_a_request_body_nothing_takes_raises_net_write_timeout
    mock = mock_resolving({ "held.test" => "93.184.216.34" })
    sandbox = mock_sandbox(mock) { |net| net.endpoint "held.test", port: 80 }
    answer = mock.hold("held.test") { sandbox.eval(UNTAKEN_POST).value }
    assert_equal "Net::WriteTimeout", answer,
                 "Net::HTTP#post of a megabyte body nothing takes, with write_timeout 0.01, must raise " \
                 "Net::WriteTimeout once the body fills the buffers, as CRuby's does"
  end
end
