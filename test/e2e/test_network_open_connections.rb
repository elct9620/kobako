# frozen_string_literal: true

require "test_helper"

# E2E — whose count a connection, a request, or a lookup holds its place in,
# staged through a Kobako::Network::Mock whose held name keeps a lookup open.
class TestE2ENetworkOpenConnections < Minitest::Test
  include E2eGuestHelper
  include NetworkSandboxes

  PUBLIC = "93.184.216.34"

  LOOKUP = <<~RUBY
    begin
      Addrinfo.getaddrinfo(%<host>p, 80)
      "resolved"
    rescue => e
      e.class.to_s
    end
  RUBY

  def lookup(sandbox, host) = sandbox.eval(format(LOOKUP, host: host)).value

  # A Sandbox reaching each of +hosts+ through +mock+, one open connection
  # at a time, its invocations free to run on several Threads.
  def single_open_sandbox(mock, *hosts)
    mock_sandbox(mock, open_connection_limit: 1, gvl: :release) do |net|
      hosts.each { |host| net.endpoint host, port: 80 }
    end
  end

  # What a lookup of next.test answers while another invocation's lookup of
  # slow.test waits on the Mock's hold, then what the held one answers.
  def lookups_beside_a_held_one(mock, sandbox)
    held = nil
    beside = mock.hold("slow.test") do
      held = Thread.new { lookup(sandbox, "slow.test") }
      Thread.pass until mock.lookups.include?("slow.test")
      lookup(sandbox, "next.test")
    end
    [beside, held.value]
  end

  # @behavior NW-218
  def test_each_invocation_counts_its_own_open_connections
    mock = mock_resolving({ "slow.test" => PUBLIC, "next.test" => PUBLIC })
    sandbox = single_open_sandbox(mock, "slow.test", "next.test")

    assert_equal %w[resolved resolved], lookups_beside_a_held_one(mock, sandbox),
                 "with open_connection_limit 1, an invocation whose lookup is still resolving must not take the " \
                 "place of another invocation of the same Sandbox running beside it"
  end
end
