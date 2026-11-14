# frozen_string_literal: true

require "test_helper"

# E2E — a name whose resolver answers otherwise mid-run, staged through a
# Kobako::Network::Mock a Service rewrites: the guest keeps what its first
# lookup answered, so a resolver cannot rebind the name.
class TestE2ENetworkMockRebinding < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  PUBLIC = "93.184.216.34"
  OTHER_PUBLIC = "93.184.216.35"

  REBOUND = <<~RUBY
    first = Addrinfo.getaddrinfo("app.test", 80).map(&:ip_address).uniq
    Rebind.call
    again = Addrinfo.getaddrinfo("APP.test.", 80).map(&:ip_address).uniq
    dialed = begin
      TCPSocket.new(first[0], 80)
    rescue => e
      e.class.to_s
    end
    [first, again, dialed]
  RUBY

  # A Sandbox whose Mock answers `app.test` with PUBLIC until the guest
  # calls the Rebind Service, then with OTHER_PUBLIC, recording each name
  # its resolve policy is asked into +asked+.
  def rebinding_sandbox(mock, asked)
    sandbox = mock_sandbox(mock) do |net|
      add_tcp_hosts(net, "app.test")
      net.policy(:resolve) { |name| asked << name }
    end
    sandbox.bind("Rebind", -> { mock.answer("app.test", OTHER_PUBLIC) })
  end

  # @behavior NW-027
  def test_a_connect_dials_what_the_lookup_answered
    mock = mock_answering({ "app.test" => PUBLIC })
    _, _, dialed = rebinding_sandbox(mock, []).eval(REBOUND).value

    assert_equal ["Errno::ECONNREFUSED", [[PUBLIC, 80]]], [dialed, mock.dials],
                 "a connect to a name's virtual address must dial what its lookup answered, even after the " \
                 "resolver answers otherwise"
  end

  # @behavior NW-175
  def test_a_name_keeps_its_virtual_address_for_the_invocation
    mock = mock_answering({ "app.test" => PUBLIC })
    asked = []
    first, again, = rebinding_sandbox(mock, asked).eval(REBOUND).value

    assert_equal [["198.18.0.0"], ["198.18.0.0"], ["app.test"], %w[app.test app.test]],
                 [first, again, mock.lookups, asked],
                 "a name looked up again in another spelling, after the resolver answers otherwise, must answer " \
                 "the same virtual address without asking the resolver, its resolve policy asked each time"
  end
end
