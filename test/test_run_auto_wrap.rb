# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Sandbox#run host→guest argument auto-wrap
# (docs/behavior.md B-34) — non-wire-representable args / kwargs values
# are routed through the Sandbox's Catalog::Handles and arrive in the guest
# as Kobako::Handle proxies whose method calls dispatch back as
# transport calls (B-17). The forged-Handle reject path (E-29) lives in
# test/test_sandbox_run.rb alongside the rest of the #run pre-flight
# error coverage; this file is the e2e elevation of the auto-wrap
# happy path against the real data/kobako.wasm.
class TestRunAutoWrap < Minitest::Test
  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
  end

  # A StringIO arrives as a positional argument. The host wraps it as
  # a Handle; the guest receives a proxy at the same arg position and
  # +#read+ on the proxy round-trips to the host StringIO.
  def test_positional_stringio_round_trips_via_handle_proxy
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Echo = ->(body) { body.read.upcase }", name: :Echo)

    assert_equal "HELLO WORLD", sandbox.run(:Echo, StringIO.new("hello world"))
  end

  # Same auto-wrap path through the kwargs branch — exercises the
  # symmetric deep_wrap walk over Hash values.
  def test_kwargs_value_stringio_round_trips_via_handle_proxy
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "App = ->(opts) { opts[:body].read }", name: :App)

    assert_equal "payload", sandbox.run(:App, body: StringIO.new("payload"))
  end
end
