# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Sandbox#run host→guest argument auto-wrap
# (docs/behavior/dispatch.md B-34) — non-wire-representable args / kwargs values
# are routed through the Sandbox's Catalog::Handles and arrive in the guest
# as Kobako::Handle proxies whose method calls dispatch back as
# transport calls (B-17). The forged-Handle reject path (E-29) lives in
# test/sandbox/test_run.rb alongside the rest of the #run pre-flight
# error coverage; this file is the e2e elevation of the auto-wrap
# happy path against the real data/kobako.wasm.
class TestRunAutoWrap < Minitest::Test
  include E2eGuestHelper

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

  # Auto-wrap applies to Hash values, not keys: a non-wire-representable
  # object may cross as a value (above) but not as a key. #run rejects such
  # a key with a public SandboxError rather than leaking the internal codec
  # UnsupportedType that a raw encode would otherwise raise.
  def test_non_representable_hash_key_argument_is_rejected_as_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "App = ->(h) { h.size }", name: :App)

    assert_raises(Kobako::SandboxError) do
      sandbox.run(:App, { StringIO.new("k") => "v" })
    end
  end

  # A cyclic argument nests without bound and cannot faithfully cross. The
  # host refuses it while encoding the run envelope (E-54), so #run surfaces a
  # clean SandboxError before entering the guest rather than a host stack
  # overflow escaping the invocation.
  def test_cyclic_argument_is_rejected_as_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "App = ->(x) { x }", name: :App)
    cyclic = []
    cyclic << cyclic

    assert_raises(Kobako::SandboxError) { sandbox.run(:App, cyclic) }
  end
end
