# frozen_string_literal: true

require "test_helper"

# E2E — a script's Net::HTTP over TLS through the real Guest Binary: TLS ends
# on the host, so a server that cannot speak it fails as CRuby's does.
class TestE2ENetworkHttps < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  OVER_TLS = <<~RUBY
    begin
      Net::HTTP.start("127.0.0.1", %<port>d, use_ssl: true) { |http| http.get("/") }
    rescue OpenSSL::SSL::SSLError => e
      [e.class.to_s, e.is_a?(OpenSSL::OpenSSLError)]
    end
  RUBY

  # A loopback server answering in clear the moment it accepts, whatever the
  # client sends first.
  def with_clear_server
    server = TCPServer.new("127.0.0.1", 0)
    thread = Thread.new do
      client = server.accept
      client.write("HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
      client.close
    end
    yield server.addr[1]
  ensure
    thread&.join(5)
    server&.close
  end

  # @behavior NW-049
  def test_tls_to_a_server_speaking_clear_raises_ssl_error
    with_clear_server do |port|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:web) { |net| net.endpoint "127.0.0.1", port: port }

      assert_equal ["OpenSSL::SSL::SSLError", true], sandbox.eval(format(OVER_TLS, port: port)).value,
                   "Net::HTTP with use_ssl to a server speaking clear must raise OpenSSL::SSL::SSLError"
    end
  end
end
