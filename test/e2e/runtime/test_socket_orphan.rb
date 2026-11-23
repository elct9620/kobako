# frozen_string_literal: true

require "test_helper"

# E2E — a TCP socket a script closes while bytes it wrote still wait for a
# peer that never reads, through the real Guest Binary: the socket goes on
# sending as Linux's orphan does, holding its connection but no descriptor.
class TestE2ESocketOrphan < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  CLOSES_STILL_SENDING = <<~RUBY
    socket = TCPSocket.new("127.0.0.1", %<port>d)
    4096.times { break unless socket.write_nonblock("x" * 65536, exception: false).is_a?(Integer) }
    socket.close
    opened = File.open("/data/note.txt") { |file| file.read }
    again = begin
      TCPSocket.new("127.0.0.1", %<port>d)
    rescue SystemCallError => e
      e.class.to_s
    end
    [opened, again]
  RUBY

  # The three standard descriptors and the root preopen leave a cap of 5
  # room for the socket alone.
  # @behavior RT-415
  def test_a_socket_closed_still_sending_keeps_its_connection_not_its_descriptor
    with_listening_server do |port|
      sandbox = loopback_sandbox(port, open_file_limit: 5, open_connection_limit: 1)
                .mount("/data", Kobako::Filesystem::Tmpfs.new("note.txt" => "hello"))

      assert_equal ["hello", "Errno::EAGAIN"], sandbox.eval(format(CLOSES_STILL_SENDING, port: port)).value,
                   "TCPSocket#close with bytes a peer never reads, under open_file_limit: 5 and " \
                   "open_connection_limit: 1, must let File.open take the descriptor at once while " \
                   "TCPSocket.new raises Errno::EAGAIN until the socket closes"
    end
  end
end
