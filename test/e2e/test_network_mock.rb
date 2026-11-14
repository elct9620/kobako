# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox built with network: Kobako::Network::Mock, its guest naming
# hosts the Mock answers while a loopback server the test runs takes the
# dials it routes.
class TestE2ENetworkMock < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  PUBLIC = "93.184.216.34"
  OTHER_PUBLIC = "93.184.216.35"

  # Each host's TCPSocket, answering what the server sent back for "ping",
  # or the class of what the connect raised.
  CONNECT_EACH = <<~RUBY
    %<hosts>p.map do |host|
      socket = TCPSocket.new(host, 80)
      socket.write("ping")
      socket.close_write
      socket.read
    rescue => e
      e.class.to_s
    end
  RUBY

  def connect_each(mock, *hosts)
    mock_sandbox(mock) { |net| add_tcp_hosts(net, *hosts) }.eval(format(CONNECT_EACH, hosts: hosts)).value
  end

  # A Mock answering +name+ with PUBLIC, routed to the loopback +port+.
  def routed_mock(name, port)
    mock_answering({ name => PUBLIC }).tap { |mock| mock.route(PUBLIC, 80, to: ["127.0.0.1", port]) }
  end

  # @behavior NW-212
  def test_a_mock_answers_names_from_its_table_and_dials_only_its_routes
    with_upcase_server do |port|
      mock = routed_mock("app.test", port)
      mock.answer("unrouted.test", OTHER_PUBLIC)

      assert_equal [%w[PING Errno::ECONNREFUSED], %w[app.test unrouted.test], [[PUBLIC, 80], [OTHER_PUBLIC, 80]]],
                   [connect_each(mock, "app.test", "unrouted.test"), mock.lookups, mock.dials],
                   "a Sandbox built with network: a Mock must answer each name from the Mock's table, reach the " \
                   "server a routed address leads to, refuse an unrouted one, and read back each name and dial"
    end
  end

  # @behavior NW-213
  def test_a_mock_refuses_a_dial_with_the_error_it_was_given
    mock = mock_answering({ "app.test" => PUBLIC })
    mock.refuse(PUBLIC, 80, Errno::ECONNRESET)

    assert_equal ["Errno::ECONNRESET"], connect_each(mock, "app.test"),
                 "a Mock told to refuse an address with Errno::ECONNRESET must fail the guest's connect " \
                 "as a host socket reset does"
  end

  # @behavior NW-214
  def test_a_mocks_held_name_answers_once_its_hold_ends
    with_upcase_server do |port|
      mock = routed_mock("held.test", port)
      waited, run = mock.hold("held.test") { connect_while_held(mock) }

      assert_equal [true, ["PING"]], [waited, run.value],
                   "a lookup of a name a Mock holds must wait while hold's block runs, then connect once it returns"
    end
  end

  # Start the guest's connect to `held.test` on a Thread, answering whether
  # it still waits once its lookup reached +mock+, and the Thread.
  def connect_while_held(mock)
    running = Thread.new { connect_each(mock, "held.test") }
    sleep 0.01 until mock.lookups.any? || !running.alive?
    [running.join(0.2).nil?, running]
  end

  def test_refuse_with_an_errno_kobako_never_answers_raises_argument_error
    assert_raises(ArgumentError, "Mock#refuse with Errno::ECHILD, an errno no socket answers, must raise " \
                                 "ArgumentError") do
      Kobako::Network::Mock.new.refuse(PUBLIC, 80, Errno::ECHILD)
    end
  end

  def test_hold_without_a_block_raises_argument_error
    assert_raises(ArgumentError, "Mock#hold without a block must raise ArgumentError, as a hold lasts for its block") do
      Kobako::Network::Mock.new.hold("held.test")
    end
  end

  def test_a_network_option_other_than_a_mock_raises_type_error
    assert_raises(TypeError, "Sandbox.new with network: other than nil or a Mock must raise TypeError") do
      Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM, network: Object.new).eval("1")
    end
  end
end
