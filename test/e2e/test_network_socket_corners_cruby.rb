# frozen_string_literal: true

require "test_helper"

# E2E — the edges where a script's socket answers as CRuby's on Linux, through
# the real Guest Binary against loopback peers: an empty datagram read as the
# end of one read, a bind naming no host, option levels and short values, and
# port 0 that WASI calls invalid.
class TestE2ENetworkSocketCornersCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback
  include UdpLoopback

  # Answer each "go" with an empty datagram and each "more" with "next".
  def with_empty_datagram_peer
    peer = UDPSocket.new
    peer.bind("127.0.0.1", 0)
    answer = ->(mesg, from) { peer.send(mesg == "go" ? "" : "next", 0, from[3], from[1]) }
    thread = Thread.new { loop { answer.call(*peer.recvfrom(16)) } }
    yield peer.addr[1]
  ensure
    thread&.kill
    peer&.close
  end

  EMPTY_DATAGRAM_READS = <<~RUBY
    udp = UDPSocket.new
    udp.connect("127.0.0.1", %<port>d)
    reads = [-> { udp.readpartial(10) }, -> { udp.sysread(10) }, -> { udp.read(5) }, -> { udp.gets },
             -> { udp.eof? }, -> { udp.read_nonblock(10, exception: false) }, -> { udp.readline }]
    answers = reads.map do |read|
      udp.send("go", 0)
      udp.wait_readable(5)
      read.call
    rescue EOFError => e
      e.class.to_s
    end
    udp.send("more", 0)
    answers + [udp.readpartial(10)]
  RUBY

  # @behavior NW-223
  def test_an_empty_datagram_reads_as_the_end_of_one_read
    with_empty_datagram_peer do |port|
      assert_equal ["EOFError", "EOFError", nil, nil, true, nil, "EOFError", "next"],
                   udp_sandbox(port).eval(format(EMPTY_DATAGRAM_READS, port: port)).value,
                   "a UDPSocket's readpartial, sysread, and readline must raise EOFError on an empty datagram, " \
                   "read and gets answer nil, eof? true, and read_nonblock nil, as CRuby reads read(2)'s 0, and " \
                   "the next read must take the next datagram"
    end
  end

  NAMELESS_BIND = <<~RUBY
    begin
      UDPSocket.new.bind(nil, 0)
    rescue SystemCallError => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-117
  def test_a_bind_naming_no_host_takes_the_loopback
    with_quiet_udp_peer do |port, _peer|
      assert_equal "Errno::EACCES", udp_sandbox(port).eval(NAMELESS_BIND).value,
                   "UDPSocket#bind(nil, 0) must take the loopback, as CRuby's rsock_addrinfo reads a host it is " \
                   "not given with no AI_PASSIVE, and the host refuses a bind to the loopback"
    end
  end

  OPTION_EDGES = <<~RUBY
    answer = lambda do |&call|
      call.call
    rescue StandardError => e
      e.class.to_s
    end
    tcp = TCPSocket.new("127.0.0.1", %<port>d)
    udp = UDPSocket.new
    [answer.call { tcp.setsockopt(999, 1, 1) }, answer.call { tcp.setsockopt(Socket::IPPROTO_UDP, 1, 1) },
     answer.call { udp.setsockopt(999, 1, 1) }, answer.call { tcp.getsockopt(999, 1) },
     answer.call { tcp.setsockopt(:IP, :TTL, "\\x07"); tcp.getsockopt(:IP, :TTL).int },
     answer.call { tcp.setsockopt(:IP, :TTL, "ab"); tcp.getsockopt(:IP, :TTL).int },
     answer.call { tcp.setsockopt(:IP, :TTL, "") }, answer.call { tcp.setsockopt(:SOCKET, :KEEPALIVE, "\\x01") }]
  RUBY

  # @behavior NW-126
  def test_an_option_level_and_a_short_value_answer_as_linuxs
    with_listening_server do |port|
      assert_equal ["Errno::ENOPROTOOPT", "Errno::ENOPROTOOPT", "Errno::ENOPROTOOPT", "Errno::ENOTSUP", 7, 97,
                    "Errno::EINVAL", "Errno::EINVAL"],
                   loopback_sandbox(port).eval(format(OPTION_EDGES, port: port)).value,
                   "setsockopt at a level a socket does not take must raise ENOPROTOOPT and getsockopt ENOTSUP, " \
                   "and an IP option shorter than an int must read its first byte, as Linux's do_ip_setsockopt; an " \
                   "empty value, or a short SOL_SOCKET one, raises EINVAL"
    end
  end

  PORT_ZERO = <<~RUBY
    [-> { TCPSocket.new("127.0.0.1", 0) }, -> { UDPSocket.new.connect("127.0.0.1", 0) }].map do |call|
      call.call
    rescue SystemCallError => e
      e.class.to_s
    end
  RUBY

  # @behavior NW-099
  def test_port_zero_raises_einval_as_wasi_calls_it_invalid
    with_listening_server do |port|
      assert_equal %w[Errno::EINVAL Errno::EINVAL], loopback_sandbox(port).eval(PORT_ZERO).value,
                   "TCPSocket.new and UDPSocket#connect to port 0 must raise Errno::EINVAL, where CRuby's dials " \
                   "it, since WASI answers invalid-argument for port 0"
    end
  end
end
