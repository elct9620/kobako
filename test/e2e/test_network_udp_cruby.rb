# frozen_string_literal: true

require "test_helper"

# E2E — the UDPSocket surface a script shares with CRuby, through the real
# Guest Binary against a loopback peer: the reads that share what a socket
# holds, and the calls CRuby's refuses.
class TestE2ENetworkUdpCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback
  include UdpLoopback

  BOUND = <<~RUBY
    tcp = TCPSocket.new("127.0.0.1", %<tcp>d, "0.0.0.0", 0)
    tcp.write("ping")
    tcp.close_write
    any = UDPSocket.new
    bound = any.bind("<any>", "0")
    named = begin; UDPSocket.new.bind("127.0.0.1", 0); rescue Errno::EACCES; :refused; end
    [tcp.read, bound, named]
  RUBY

  # @behavior NW-117
  def test_a_socket_binds_the_local_end_the_host_allows
    with_upcase_server do |tcp|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:local) do |net|
        net.endpoint "127.0.0.1", port: tcp, protocol: :tcp, addresses: [:local]
      end

      assert_equal ["PING", 0, :refused], sandbox.eval(format(BOUND, tcp: tcp)).value,
                   "TCPSocket.new with local end \"0.0.0.0\" port 0 must connect and UDPSocket#bind to " \
                   "\"<any>\" port \"0\" pass, as CRuby reads each with AI_PASSIVE, while a bind to " \
                   "127.0.0.1 raises Errno::EACCES"
    end
  end

  READ = <<~RUBY
    socket = UDPSocket.new
    socket.connect("127.0.0.1", %<port>d)
    socket.send("ping", 0)
    [socket.read(2), socket.read(2)]
  RUBY

  # @behavior NW-095
  def test_a_udp_socket_reads_one_whole_datagram_through_what_it_holds
    with_upcase_peer do |port|
      value = sandbox_sending_to(port).eval(format(READ, port: port)).value

      assert_equal %w[PI NG], value,
                   "UDPSocket#read(2) must take the peer's whole datagram, answering its first two bytes and " \
                   "holding back the rest for the next read, as CRuby's IO does"
    end
  end

  REFUSES = <<~RUBY
    refused = lambda do |&call|
      call.call
      :passed
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
    socket = UDPSocket.new
    [
      refused.call { UDPSocket.new(99) }.first,
      refused.call { UDPSocket.new(:INET6) }.first,
      refused.call { UDPSocket.new("PF_INET").close },
      refused.call { UDPSocket.new("FOO") },
      refused.call { UDPSocket.new(nil) }.first,
      refused.call { UDPSocket.new(Socket::AF_INET, 0) }.last,
      refused.call { socket.send("ping") }.last,
      refused.call { socket.recv }.last,
      refused.call { Socket.new(2, 1) }.first
    ]
  RUBY

  CRUBY_REFUSES = ["Errno::EAFNOSUPPORT", "Errno::EAFNOSUPPORT", :passed,
                   ["SocketError", "unknown socket domain: FOO"], "TypeError",
                   "wrong number of arguments (given 2, expected 0..1)",
                   "wrong number of arguments (given 1, expected 2..4)",
                   "wrong number of arguments (given 0, expected 1..3)", "NoMethodError"].freeze

  # @behavior NW-098
  def test_a_udp_socket_refuses_what_crubys_refuses
    assert_equal CRUBY_REFUSES, sandbox_sending_to(9).eval(REFUSES).value,
                 "UDPSocket.new of a family other than AF_INET by number or name must raise " \
                 "Errno::EAFNOSUPPORT, PF_INET must open, an unknown name raise SocketError and nil TypeError, " \
                 "as rsock_family_arg reads each; UDPSocket.new, " \
                 "#send, and #recv with a wrong argument count must raise ArgumentError naming CRuby's range; " \
                 "and Socket.new must raise NoMethodError"
  end

  PAST_THE_LIMIT = <<~RUBY
    held = []
    begin
      20.times { held << UDPSocket.new }
      :all_opened
    rescue SystemCallError => e
      [e.class.to_s, e.message]
    end
  RUBY

  # @behavior NW-069
  def test_a_socket_past_the_open_file_limit_raises_emfile
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, open_file_limit: 8).network(:local) do |net|
      net.endpoint "127.0.0.1", port: 9, protocol: :udp, addresses: [:local]
    end

    assert_equal ["Errno::EMFILE", "Too many open files - socket(2) - udp"], sandbox.eval(PAST_THE_LIMIT).value,
                 "UDPSocket.new past a Sandbox's open_file_limit must raise Errno::EMFILE, as CRuby's socket(2) does"
  end
end
