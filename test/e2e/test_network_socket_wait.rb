# frozen_string_literal: true

require "test_helper"

# E2E — a script waiting on its socket, through the real Guest Binary
# against a loopback server: before bytes come, once they come, once a read
# holds some back, and once the socket is closed.
class TestE2ENetworkSocketWait < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  WAITS = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    before = [socket.wait_readable(0), socket.wait_writable(0).equal?(socket)]
    socket.write("\\r\\n")
    after = [socket.wait_readable.equal?(socket), socket.gets, socket.wait_readable(0)]
    socket.close
    closed = begin
      socket.wait_readable(0)
    rescue IOError => e
      e.message
    end
    [before, after, closed]
  RUBY

  # @behavior NW-132
  def test_a_script_waits_on_a_socket_as_crubys
    with_answering_server("a\nb") do |port|
      value = loopback_sandbox(port).eval(format(WAITS, port: port)).value

      assert_equal [[nil, true], [true, "a\n", true], "closed stream"], value,
                   "TCPSocket#wait_readable(0) before bytes come must answer nil and #wait_writable(0) the " \
                   "socket; once bytes come #wait_readable must answer the socket, true while a #gets holds bytes " \
                   "back, and raise IOError once closed, as CRuby's do"
    end
  end
end
