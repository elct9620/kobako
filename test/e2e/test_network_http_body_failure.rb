# frozen_string_literal: true

require "test_helper"

# E2E — how a Net::HTTP request body that fails on the host while it is
# written reaches the script, staged through a Kobako::Network::Mock.
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
                 "Errno::ENOMEM, as a socket's write past the limit does"
  end
end
