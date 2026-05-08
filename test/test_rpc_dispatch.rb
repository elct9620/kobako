# frozen_string_literal: true

require "test_helper"

# Item #18 — guest-initiated RPC dispatch end-to-end against the
# `test/fixtures/test-guest.wasm` fixture.
#
# The fixture's `__kobako_run` recognises `rpc:Group::Member|method|argument`
# source string. When invoked it builds a Request envelope, calls the host
# import `__kobako_rpc_call`, decodes the Response, and embeds the outcome
# in a Result envelope so the host test can assert the round-trip.
#
#   * Response.ok(Value::Str("HELLO"))  → Result(Value::Str("HELLO"))
#   * Response.err(<exception bytes>)   → Result(Value::Str("err:<N>bytes"))
#
# These tests pin the wiring SPEC §B-12 (target string `"Group::Member"`
# dispatch) and §B-13 (positional + kwargs argument unwrap) demand: a
# Sandbox with a bound Service Member; the host import dispatches; the
# Response value flows back through the guest's outcome envelope.
#
# Service Member names must be constant-form (SPEC §B-08); the bound
# objects below are lambdas, dispatched by their `call` method.
class TestRpcDispatch < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/test-guest.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Engine)
    skip "test-guest fixture missing (run `bundle exec rake fixtures:test_guest`)" \
      unless File.exist?(FIXTURE_PATH)
  end

  def test_dispatches_string_target_to_bound_service_member_and_returns_value
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.define(:Logger).bind(:Echo, lambda(&:upcase))

    assert_equal "HELLO", sandbox.run("rpc:Logger::Echo|call|hello")
  end

  def test_dispatches_to_a_different_member_in_the_same_group
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.define(:Logger).bind(:Echo, ->(arg) { "echoed:#{arg}" })

    assert_equal "echoed:abc", sandbox.run("rpc:Logger::Echo|call|abc")
  end

  def test_unknown_target_returns_response_err_to_guest
    # SPEC E-12: target path that does not match any registered Service
    # Member is reified as Response.err(type="undefined"). The guest's
    # `rpc:` branch surfaces err-branch responses as `err:<N>bytes` in a
    # Result envelope, so the host test sees a normal return value rather
    # than a SandboxError raise.
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    value = sandbox.run("rpc:Missing::Method|call|x")

    assert_kind_of String, value
    assert_match(/\Aerr:\d+bytes\z/, value)
  end

  def test_unknown_member_within_known_group_returns_response_err
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.define(:Logger).bind(:Echo, ->(arg) { arg })

    value = sandbox.run("rpc:Logger::Other|call|x")

    assert_kind_of String, value
    assert_match(/\Aerr:\d+bytes\z/, value)
  end

  def test_host_method_raise_is_reified_as_response_err
    # SPEC E-11: a bound Service method that raises is captured as
    # Response.err(type="runtime"); the guest sees the err branch.
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.define(:Boom).bind(:Bang, ->(_arg) { raise "host error" })

    value = sandbox.run("rpc:Boom::Bang|call|x")

    assert_kind_of String, value
    assert_match(/\Aerr:\d+bytes\z/, value)
  end

  def test_dispatcher_is_invoked_through_wasmtime_import_callback
    # Sanity: the RPC store-side counter increments — proves the actual
    # Rust import callback fired (no in-Ruby short circuit).
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.define(:Logger).bind(:Echo, ->(arg) { arg })

    assert_equal 0, sandbox.store.rpc_call_count
    sandbox.run("rpc:Logger::Echo|call|probe")

    assert_equal 1, sandbox.store.rpc_call_count
  end

  def test_non_rpc_runs_do_not_consume_dispatcher
    # Source paths that never call `__kobako_rpc_call` must complete
    # without touching the dispatcher (cross-check that the host wiring
    # is opt-in per call, not eager).
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.define(:Logger).bind(:Echo, ->(arg) { arg })

    sandbox.run("42")

    assert_equal 0, sandbox.store.rpc_call_count
  end
end

# Unit-level coverage of the dispatcher itself, free of the wasm fixture —
# fast and deterministic, exercises the Registry/Wire integration directly.
class TestRpcDispatcherUnit < Minitest::Test
  def setup
    @registry = Kobako::Service::Registry.new
    @handle_table = Kobako::HandleTable.new
    @dispatcher = Kobako::RpcDispatcher.new(registry: @registry, handle_table: @handle_table)
  end

  def test_dispatches_string_target_and_returns_response_ok_bytes
    @registry.define(:Logger).bind(:Echo, lambda(&:upcase))
    req = encode_request("Logger::Echo", "call", ["hi"], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.ok?
    assert_equal "HI", resp.payload
  end

  def test_passes_kwargs_as_symbols_to_bound_object
    capture = []
    klass = Class.new do
      define_method(:tag) do |arg, key:|
        capture << [arg, key]
        "ok"
      end
    end
    @registry.define(:Logger).bind(:Tag, klass.new)
    req = encode_request("Logger::Tag", "tag", ["x"], { "key" => "value" })

    resp = decode_response(@dispatcher.call(req))

    assert resp.ok?
    assert_equal [%w[x value]], capture
  end

  def test_unknown_target_returns_undefined_exception
    req = encode_request("Missing::Method", "call", ["x"], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.err?
    assert_equal "undefined", resp.payload.type
  end

  def test_method_raise_returns_runtime_exception
    @registry.define(:Boom).bind(:Bang, ->(_) { raise "boom" })
    req = encode_request("Boom::Bang", "call", ["x"], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.err?
    assert_equal "runtime", resp.payload.type
    assert_match(/boom/, resp.payload.message)
  end

  def test_argument_error_returns_argument_exception
    @registry.define(:Service).bind(:M, ->(_a, _b) { :ok })
    # Missing argument — Ruby ArgumentError on dispatch.
    req = encode_request("Service::M", "call", [], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.err?
    assert_equal "argument", resp.payload.type
  end

  private

  def encode_request(target, method, args, kwargs)
    Kobako::Wire::Envelope.encode_request(target, method, args, kwargs)
  end

  def decode_response(bytes)
    Kobako::Wire::Envelope.decode_response(bytes)
  end
end
