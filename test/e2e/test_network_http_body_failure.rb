# frozen_string_literal: true

require "socket"
require "test_helper"

# E2E — how a Net::HTTP request body, which streams to the server after the
# request leaves, reaches the script when its write fails: one a TRACE may
# not carry, and one a loopback server never reads.
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

  UNREAD_POST = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    http.write_timeout = 0.01
    http.read_timeout = 5
    begin
      http.post("/", "x" * 1_000_000)
    rescue Timeout::Error => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-238
  def test_a_request_body_the_server_never_reads_raises_net_write_timeout
    server = TCPServer.new("127.0.0.1", 0)
    # A small receive buffer, so the server's window holds little of the body.
    server.setsockopt(Socket::SOL_SOCKET, Socket::SO_RCVBUF, 4096)
    port = server.addr[1]
    assert_equal "Net::WriteTimeout", http_sandbox(port).eval(format(UNREAD_POST, port: port)).value,
                 "Net::HTTP#post of a megabyte body to a server that never reads it, with write_timeout 0.01, " \
                 "must raise Net::WriteTimeout once the body fills the buffers, as CRuby's does"
  ensure
    server&.close
  end
end
