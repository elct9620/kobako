# frozen_string_literal: true

require "test_helper"

# E2E — a fork after a guest's HTTPS, through the real guest binary. macOS
# verifies a certificate through services a forked child loses, so a fork
# there raises; Linux reads certificates from files. The HTTPS reaches a
# loopback server with a certificate of its own, which the system still
# verifies before refusing it.
class TestE2EForkAfterHttps < Minitest::Test
  include ForkingChild

  # A loopback TLS server with a certificate no system trusts, the sandbox
  # +web+ that reaches it, and the +request+ its guest makes over HTTPS.
  HTTPS_PRELUDE = <<~RUBY
    require "openssl"
    require "socket"
    key = OpenSSL::PKey::RSA.new(2048)
    cert = OpenSSL::X509::Certificate.new
    cert.version = 2
    cert.serial = 1
    cert.subject = cert.issuer = OpenSSL::X509::Name.parse("/CN=127.0.0.1")
    cert.public_key = key.public_key
    cert.not_before = Time.now - 60
    cert.not_after = Time.now + 3600
    cert.sign(key, OpenSSL::Digest.new("SHA256"))
    context = OpenSSL::SSL::SSLContext.new
    context.cert = cert
    context.key = key
    tcp = TCPServer.new("127.0.0.1", 0)
    port = tcp.addr[1]
    server = OpenSSL::SSL::SSLServer.new(tcp, context)
    Thread.new do
      loop do
        server.accept.close
      rescue OpenSSL::SSL::SSLError, SystemCallError
        next
      end
    end
    web = Kobako::Sandbox.new(wasm_path: WASM).network(:web) { |net| net.endpoint "127.0.0.1", port: port }
    request = format(<<~GUEST, port: port)
      begin
        Net::HTTP.start("127.0.0.1", %<port>d, use_ssl: true) { |http| http.get("/") }
      rescue => e
        e.class.to_s
      end
    GUEST
  RUBY

  FORK_AFTER_HTTPS = <<~RUBY.freeze
    #{HTTPS_PRELUDE}
    web.eval(request)
    print fork_outcome.inspect
  RUBY

  # @behavior S-240
  def test_a_fork_on_macos_after_a_guests_https_raises
    outcome = run_forking_child(FORK_AFTER_HTTPS)
    if RUBY_PLATFORM.include?("darwin")
      assert_match(/\A\[:raised, ".*127\.0\.0\.1.*"\]\z/, outcome,
                   "Kernel#fork after a guest's HTTPS to 127.0.0.1 on macOS must raise RuntimeError naming it")
    else
      assert_equal ":forked", outcome, "Kernel#fork after a guest's HTTPS off macOS must fork as usual"
    end
  end

  TELLS_PITCHFORK = <<~RUBY.freeze
    module Pitchfork
      module Info
        @told = 0
        def self.no_longer_fork_safe! = @told += 1
        def self.told = @told
      end
    end
    #{HTTPS_PRELUDE}
    before = Pitchfork::Info.told
    2.times { web.eval(request) }
    print [before, Pitchfork::Info.told].inspect
  RUBY

  # @behavior S-241
  def test_a_guests_https_on_macos_tells_pitchfork_once
    skip "only macOS verifies through services a forked child loses" unless RUBY_PLATFORM.include?("darwin")
    assert_equal "[0, 1]", run_forking_child(TELLS_PITCHFORK),
                 "two Sandbox#eval runs whose guest makes HTTPS on macOS must call " \
                 "Pitchfork::Info.no_longer_fork_safe! once, after the first run"
  end
end
