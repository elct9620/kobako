# frozen_string_literal: true

require "test_helper"

# E2E — how a Net::HTTP request body, which the host holds whole before the
# request leaves, reaches the script: one past the memory limit, staged
# through a Kobako::Network::Mock, and one a loopback server never reads.
class TestE2ENetworkHttpBodyFailure < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  # A body of a megabyte, which the request body's stream holds for the host
  # past what the memory limit leaves.
  LARGE_POST = <<~RUBY
    begin
      Net::HTTP.new("web.test", 80).post("/", "x" * 1_000_000)
    rescue SystemCallError, IOError => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-230
  def test_a_request_body_past_the_memory_limit_raises_enomem
    mock = mock_resolving({ "web.test" => "93.184.216.34" })
    sandbox = mock_sandbox(mock, memory_limit: 2_400_000) { |net| net.endpoint "web.test", port: 80 }
    assert_equal "Errno::ENOMEM", sandbox.eval(LARGE_POST).value,
                 "Net::HTTP#post of a body the memory limit cannot hold through Sandbox#eval must raise " \
                 "Errno::ENOMEM, as the request holds its body against the limit"
  end

  UNREAD_POST = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    http.write_timeout = 0.01
    http.read_timeout = 0.3
    begin
      http.post("/", "x" * 1_000_000)
    rescue Timeout::Error => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-238
  def test_a_request_body_the_server_never_reads_raises_net_read_timeout
    with_listening_server do |port|
      assert_equal "Net::ReadTimeout", http_sandbox(port).eval(format(UNREAD_POST, port: port)).value,
                   "Net::HTTP#post of a megabyte body to a server that never reads it, with write_timeout 0.01, " \
                   "must raise Net::ReadTimeout at its read_timeout, never Net::WriteTimeout"
    end
  end
end
