# frozen_string_literal: true

require "test_helper"

# E2E — a script's TCP socket read and written without waiting, through the
# real Guest Binary against a loopback server: what answers before bytes
# come, what a buffer holds back, the end, and a full socket.
class TestE2ENetworkSocketNonblock < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback
  include UdpLoopback

  READS = <<~RUBY.freeze
    #{NonblockOutcome::SCRIPT}
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    before = [
      outcome.call { socket.read_nonblock(10) }, socket.read_nonblock(10, exception: false),
      outcome.call { socket.recv_nonblock(10) }, socket.recv_nonblock(10, exception: false)
    ]
    socket.write("\\r\\n")
    IO.select([socket])
    buffer = +"zz"
    held = [socket.gets, outcome.call { socket.recv_nonblock(10) }, socket.read_nonblock(1, buffer), buffer]
    rest = socket.read_nonblock(10)
    IO.select([socket])
    ended = [
      outcome.call { socket.read_nonblock(10) }, socket.read_nonblock(10, exception: false),
      socket.recv_nonblock(10)
    ]
    [before, held, rest, ended]
  RUBY

  READ_ANSWERS = [
    [NonblockOutcome.would_block("read would block"), :wait_readable,
     NonblockOutcome.would_block("recvfrom(2) would block"), :wait_readable],
    ["ab\n", NonblockOutcome.plain_raise("IOError", "recvfrom for buffered IO"), "c", "c"],
    "d",
    [NonblockOutcome.plain_raise("EOFError", "end of file reached"), nil, nil]
  ].freeze

  # @behavior NW-129
  def test_a_tcp_socket_reads_without_waiting_as_crubys
    with_answering_server("ab\ncd") do |port|
      value = loopback_sandbox(port).eval(format(READS, port: port)).value

      assert_equal READ_ANSWERS, value,
                   "TCPSocket#read_nonblock and #recv_nonblock before bytes come must raise IO::EAGAINWaitReadable " \
                   "or answer :wait_readable with exception: false; after #gets, #read_nonblock must take what it " \
                   "held back and #recv_nonblock raise IOError; at the end #read_nonblock must raise EOFError and " \
                   "#recv_nonblock answer nil, as CRuby's do"
    end
  end

  WRITES = <<~RUBY.freeze
    #{NonblockOutcome::SCRIPT}
    socket = TCPSocket.new("127.0.0.1", %<tcp>d)
    counts = [socket.write_nonblock("hi"), socket.write_nonblock("", exception: false)]
    full = nil
    4096.times do
      full = socket.write_nonblock("x" * 65536, exception: false)
      break unless full.is_a?(Integer)
    end
    # The host may take a little more meanwhile, so a raise is awaited.
    raised = nil
    4096.times do
      raised = outcome.call { socket.write_nonblock("x" * 65536) }
      break unless raised.is_a?(Integer)
    end
    full = [full, raised]
    socket.close_write
    closed = outcome.call { socket.write_nonblock("x") }
    udp = UDPSocket.new
    unpeered = outcome.call { udp.write_nonblock("q") }
    udp.connect("127.0.0.1", %<udp>d)
    [counts, full, closed, udp.write_nonblock("q"), unpeered]
  RUBY

  WRITE_ANSWERS = [
    [2, 0],
    [:wait_writable, NonblockOutcome.would_block("write would block", direction: :write)],
    NonblockOutcome.plain_raise("IOError", "not opened for writing"),
    1,
    NonblockOutcome.plain_raise("Errno::EDESTADDRREQ", "Destination address required")
  ].freeze

  # @behavior NW-130
  def test_a_socket_writes_without_waiting_as_crubys
    with_listening_server do |tcp|
      with_quiet_udp_peer do |udp|
        value = tcp_and_udp_sandbox(tcp, udp).eval(format(WRITES, tcp: tcp, udp: udp)).value

        assert_equal WRITE_ANSWERS, value,
                     "#write_nonblock must answer the bytes it sent until a full TCPSocket raises " \
                     "IO::EAGAINWaitWritable or answers :wait_writable with exception: false, raise IOError after " \
                     "#close_write, answer a UDPSocket's datagram length, and raise Errno::EDESTADDRREQ with no peer"
      end
    end
  end
end
