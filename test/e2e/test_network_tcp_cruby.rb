# frozen_string_literal: true

require "test_helper"

# E2E — the TCPSocket surface a script shares with CRuby, through the real
# Guest Binary against a loopback server: the IO it is, and the call shapes
# CRuby's takes.
class TestE2ENetworkTcpCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  SELECT = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    socket.write("ping")
    silent = IO.select([socket], nil, nil, 0.05)
    socket.close_write
    answered = IO.select([socket], nil, nil, 5)
    writable = IO.select(nil, [$stdout, socket], nil, 5)
    [
      [IPSocket, BasicSocket, IO].all? { |ancestor| socket.is_a?(ancestor) },
      socket.fileno.is_a?(Integer),
      silent.nil?,
      answered && answered[0] == [socket],
      writable && writable[1] == [$stdout, socket],
      socket.read
    ]
  RUBY

  WRAP_IN_TLS = <<~RUBY
    begin
      OpenSSL::SSL::SSLSocket
      :wrapped
    rescue NameError => e
      e.message
    end
  RUBY

  # @behavior NW-080
  def test_a_tcp_socket_carries_no_tls_of_its_own
    value = sandbox_reaching(9).eval(WRAP_IN_TLS).value

    assert_equal "uninitialized constant OpenSSL::SSL::SSLSocket", value,
                 "a script with a network naming OpenSSL::SSL::SSLSocket to wrap a TCPSocket must raise NameError, " \
                 "since the guest has no TLS stack"
  end

  # @behavior NW-065
  def test_a_tcp_socket_is_an_io_that_select_waits_on
    with_upcase_server do |port|
      value = sandbox_reaching(port).eval(format(SELECT, port: port)).value

      assert_equal [true, true, true, true, true, "PING"], value,
                   "TCPSocket must descend from IPSocket, BasicSocket, and IO with an Integer fileno, and IO.select " \
                   "must time out with nil on a silent socket, answer it readable once the server writes, and " \
                   "answer $stdout beside it writable"
    end
  end

  IPV6 = <<~RUBY
    [
      (begin; TCPSocket.new("::1", %<port>d); rescue Errno::EAFNOSUPPORT; :refused; end),
      (begin; UDPSocket.new(Socket::AF_INET6); rescue Errno::EAFNOSUPPORT; :refused; end)
    ]
  RUBY

  # @behavior NW-066
  def test_an_ipv6_family_or_literal_raises_eafnosupport
    with_upcase_server do |port|
      value = sandbox_reaching(port).eval(format(IPV6, port: port)).value
      TCPSocket.new("127.0.0.1", port).close

      assert_equal %i[refused refused], value,
                   "TCPSocket to ::1 and UDPSocket.new(Socket::AF_INET6) must each raise Errno::EAFNOSUPPORT"
    end
  end

  SHAPES = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d, nil, nil, connect_timeout: 5)
    sent = socket.write("a|", "b|", "c")
    socket.close_write
    first = socket.gets("|")
    buffer = +""
    socket.readpartial(2, buffer)
    chosen = begin; TCPSocket.new("127.0.0.1", %<port>d, nil, 4321); rescue Errno::EACCES; :refused; end
    unknown = begin; TCPSocket.new("127.0.0.1", %<port>d, wait: 1); rescue ArgumentError; :refused; end
    [sent, first, buffer, socket.read, chosen, unknown]
  RUBY

  # @behavior NW-067
  def test_a_tcp_socket_takes_crubys_call_shapes
    with_upcase_server do |port|
      value = sandbox_reaching(port).eval(format(SHAPES, port: port)).value

      assert_equal [5, "A|", "B|", "C", :refused, :refused], value,
                   "TCPSocket.new with connect_timeout:, write of several strings, gets with a separator, and " \
                   "readpartial into a buffer must answer as CRuby's, while a chosen local port raises " \
                   "Errno::EACCES and an unknown keyword ArgumentError"
    end
  end
end
