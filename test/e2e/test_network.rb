# frozen_string_literal: true

require "test_helper"

# E2E — Sandbox#network and Context#network through the real Guest Binary:
# the registration rules a network entry shares with a mount.
class TestE2ENetwork < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  def api_sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:api) { |net| net.endpoint "api.example.com", port: 443 }
  end

  # @behavior NW-001
  def test_an_entry_after_the_first_invocation_is_refused
    sandbox = api_sandbox
    sandbox.eval("nil")

    assert_raises(ArgumentError, "Sandbox#network after the first invocation must be refused") do
      sandbox.network(:other) { |net| net.endpoint "other.example.com", port: 443 }
    end
  end

  # @behavior NW-005
  def test_a_context_cannot_replace_a_name_never_declared
    error = assert_raises(ArgumentError, "ctx.network of a name the Sandbox never gave must be refused") do
      api_sandbox.eval("nil") { |ctx| ctx.network(:other) { |net| net.endpoint "x.example.com", port: 443 } }
    end
    assert_includes error.message, "undeclared network entry",
                    "ctx.network of a name the Sandbox never gave must say the name is undeclared"
  end

  # @behavior NW-011
  def test_without_the_network_a_tcp_socket_raises_eacces
    value = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(<<~RUBY).value
      ["127.0.0.1", "example.com"].map do |host|
        TCPSocket.new(host, 80)
      rescue SystemCallError, SocketError => e
        [e.class.to_s, e.respond_to?(:error_code) ? e.error_code : nil]
      end
    RUBY

    assert_equal [["Errno::EACCES", nil], ["Socket::ResolutionError", -4]], value,
                 "TCPSocket.new in a Sandbox with no network entry must raise Errno::EACCES for an address " \
                 "and Socket::ResolutionError EAI_FAIL for a name"
  end

  def test_an_entry_playing_no_endpoints_lets_the_invocation_run
    value = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:audit, Object.new).eval("1 + 1").value

    assert_equal 2, value, "Sandbox#network with an object answering no endpoints must let the invocation run"
  end

  REACHED_PORTS = <<~RUBY
    [%<first>d, %<second>d].map do |port|
      TCPSocket.new("127.0.0.1", port).close
      :reached
    rescue Errno::EACCES
      :refused
    end
  RUBY

  # @behavior NW-006
  def test_a_context_replaces_an_entry_for_its_invocation_only
    with_listening_server do |own|
      with_listening_server do |replaced|
        sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:api) { |net| add_loopback_tcp(net, own) }
        script = format(REACHED_PORTS, first: own, second: replaced)
        during = sandbox.eval(script) { |ctx| ctx.network(:api) { |net| add_loopback_tcp(net, replaced) } }.value

        assert_equal [%i[refused reached], %i[reached refused]], [during, sandbox.eval(script).value],
                     "ctx.network(:api) must give that invocation the replacing entry's endpoints, and the next " \
                     "invocation the Sandbox's own"
      end
    end
  end

  # An entry answering only endpoints, each any object with the fields read.
  DuckEntry = Struct.new(:endpoints)
  HostAndPort = Struct.new(:host, :port)
  AddressedEndpoint = Struct.new(:host, :port, :addresses)

  LOCALHOST_GETS = <<~RUBY
    public_only = begin; Net::HTTP.new("localhost", %<public>d).get("/"); rescue Errno::EACCES; :refused; end
    [public_only, Net::HTTP.new("localhost", %<port>d).get("/").code]
  RUBY

  # @behavior NW-078
  def test_a_duck_typed_endpoint_takes_the_defaults_and_an_ipaddr
    with_http_server do |port, _seen|
      public_port = other_port(port)
      entry = DuckEntry.new([HostAndPort.new("localhost", public_port),
                             AddressedEndpoint.new("localhost", port, [IPAddr.new("127.0.0.0/8")])])
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:duck, entry)

      assert_equal [:refused, "201"], sandbox.eval(format(LOCALHOST_GETS, public: public_port, port: port)).value,
                   "an endpoint object answering only host and port must lead only to public addresses, and one " \
                   "naming an IPAddr must speak :http and lead where the IPAddr's prefix covers"
    end
  end
end
