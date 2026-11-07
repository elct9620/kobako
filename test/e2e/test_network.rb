# frozen_string_literal: true

require "test_helper"

# E2E — Sandbox#network and Context#network through the real Guest Binary:
# the registration rules a network entry shares with a mount.
class TestE2ENetwork < Minitest::Test
  include E2eGuestHelper

  def sandbox_with_api
    Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:api) { |net| net.endpoint "api.example.com", port: 443 }
  end

  # @behavior NW-001
  def test_an_entry_after_the_first_invocation_is_refused
    sandbox = sandbox_with_api
    sandbox.eval("nil")

    assert_raises(ArgumentError, "Sandbox#network after the first invocation must be refused") do
      sandbox.network(:other) { |net| net.endpoint "other.example.com", port: 443 }
    end
  end

  # @behavior NW-005
  def test_a_context_cannot_replace_a_name_never_declared
    error = assert_raises(ArgumentError, "ctx.network of a name the Sandbox never gave must be refused") do
      sandbox_with_api.eval("nil") { |ctx| ctx.network(:other) { |net| net.endpoint "x.example.com", port: 443 } }
    end
    assert_includes error.message, "undeclared network entry",
                    "ctx.network of a name the Sandbox never gave must say the name is undeclared"
  end

  # @behavior NW-011
  def test_without_the_network_a_tcp_socket_raises_not_implemented_error
    value = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(<<~RUBY).value
      begin
        TCPSocket.new("127.0.0.1", 80)
      rescue NotImplementedError => e
        [e.class.to_s, e.message.include?("no network")]
      end
    RUBY

    assert_equal ["NotImplementedError", true], value,
                 "TCPSocket.new in a Sandbox with no network entry must raise NotImplementedError naming it"
  end

  def test_an_entry_playing_no_endpoints_lets_the_invocation_run
    value = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:audit, Object.new).eval("1 + 1").value

    assert_equal 2, value, "Sandbox#network with an object answering no endpoints must let the invocation run"
  end

  def test_a_context_replacing_a_declared_name_runs_the_invocation
    value = sandbox_with_api.eval("1 + 1") do |ctx|
      ctx.network(:api) { |net| net.endpoint "staging.example.com", port: 443 }
    end.value

    assert_equal 2, value, "ctx.network of a declared name must let the invocation run"
  end
end
