# frozen_string_literal: true

require "test_helper"

# E2E — the kinds and protocols a script's lookup may ask for, through the real
# Guest Binary: one glibc's table has no entry for is refused before the name or
# the service is read, as CRuby's on Linux is.
class TestE2ENetworkAddrinfoKinds < Minitest::Test
  include E2eGuestHelper
  include NetworkSandboxes

  UNLISTED_KINDS = <<~RUBY
    [-> { Addrinfo.getaddrinfo("unnamed.test", "zz", nil, 99) },
     -> { Addrinfo.getaddrinfo("127.0.0.1", 80, nil, :STREAM, Socket::IPPROTO_UDP) },
     -> { Socket.getaddrinfo("127.0.0.1", 80, nil, :SEQPACKET) },
     -> { Addrinfo.new(Socket.sockaddr_in(80, "1.2.3.4"), nil, 99).family_addrinfo("1.2.3.4", 81) }].map do |call|
      call.call
    rescue SocketError => e
      [e.class.to_s, e.error_code, e.message]
    end
  RUBY

  # @behavior NW-141
  def test_a_lookup_refuses_a_kind_glibcs_table_has_no_entry_for
    refused = ["Socket::ResolutionError", -7, "getaddrinfo: ai_socktype not supported"]
    assert_equal [refused] * 4, localhost_sandbox(80).eval(UNLISTED_KINDS).value,
                 "Addrinfo.getaddrinfo, Socket.getaddrinfo, and Addrinfo#family_addrinfo asked for a kind or " \
                 "protocol glibc's table has no entry for, or SOCK_SEQPACKET, must raise Socket::ResolutionError " \
                 "with EAI_SOCKTYPE before the name or the service is read, never a Trap"
  end
end
