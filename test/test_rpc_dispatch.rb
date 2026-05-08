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

  # ---------- B-14 — host wraps stateful return values as Handles ----------

  # SPEC B-14: a Service method whose return value falls outside the wire
  # type set (B-13) is automatically allocated a HandleTable entry, and
  # the guest sees a Wire::Handle in the Response.ok payload.
  def test_non_wire_return_value_is_wrapped_as_handle
    greeter = Class.new do
      def initialize(name) = (@name = name)
      def greet(prefix = "hi") = "#{prefix},#{@name}"
    end
    @registry.define(:Factory).bind(:Make, ->(name) { greeter.new(name) })
    req = encode_request("Factory::Make", "call", ["Alice"], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.ok?
    assert_kind_of Kobako::Wire::Handle, resp.payload
    bound = @handle_table.fetch(resp.payload.id)

    assert_equal "hi,Alice", bound.greet
  end

  def test_primitive_return_value_is_not_wrapped
    @registry.define(:Logger).bind(:Echo, ->(arg) { arg })
    req = encode_request("Logger::Echo", "call", ["plain"], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.ok?
    assert_equal "plain", resp.payload
    assert_equal 0, @handle_table.size
  end

  # ---------- B-16 — guest passes Handle as argument ----------

  # SPEC B-16: a Wire::Handle arriving as an argument is resolved against
  # the HandleTable before dispatch, and the bound Service method receives
  # the live Ruby object.
  def test_handle_arg_is_resolved_to_bound_object_before_dispatch
    greeter = Class.new do
      def initialize(name) = (@name = name)
      def greet = "hello,#{@name}"
    end.new("Alice")
    handle_id = @handle_table.alloc(greeter)
    @registry.define(:Echo).bind(:Wrap, ->(g) { "wrapped:#{g.greet}" })
    req = encode_request("Echo::Wrap", "call", [Kobako::Wire::Handle.new(handle_id)], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.ok?
    assert_equal "wrapped:hello,Alice", resp.payload
  end

  def test_handle_kwarg_is_resolved_to_bound_object_before_dispatch
    obj = Object.new
    def obj.greet = "kw_ok"
    handle_id = @handle_table.alloc(obj)

    capture = []
    klass = Class.new do
      define_method(:run) do |target:|
        capture << target.greet
        "done"
      end
    end
    @registry.define(:K).bind(:Run, klass.new)
    req = encode_request("K::Run", "run", [], { "target" => Kobako::Wire::Handle.new(handle_id) })

    resp = decode_response(@dispatcher.call(req))

    assert resp.ok?
    assert_equal "done", resp.payload
    assert_equal ["kw_ok"], capture
  end

  def test_unknown_handle_arg_returns_undefined_exception
    req = encode_request("Logger::Echo", "call", [Kobako::Wire::Handle.new(999)], {})
    @registry.define(:Logger).bind(:Echo, ->(x) { x })

    resp = decode_response(@dispatcher.call(req))

    assert resp.err?
    assert_equal "undefined", resp.payload.type
  end

  # ---------- B-17 — guest passes Handle as target (chained composition) -

  # SPEC B-17: a Wire::Handle target resolves to the bound object directly;
  # the Registry is bypassed and dispatch goes straight to public_send.
  def test_handle_target_is_dispatched_to_bound_object
    obj = Class.new do
      def find(id) = "row:#{id}"
    end.new
    handle_id = @handle_table.alloc(obj)
    req = encode_request_with_target(Kobako::Wire::Handle.new(handle_id), "find", [42], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.ok?
    assert_equal "row:42", resp.payload
  end

  def test_handle_target_returning_stateful_value_is_wrapped_as_new_handle
    # B-17 + B-14 chained: invoking a Handle target whose method returns
    # another non-primitive object yields a fresh Handle in the response.
    leaf = Class.new do
      def kind = "leaf"
    end
    factory = Class.new do
      define_method(:make) { leaf.new }
    end.new
    parent_id = @handle_table.alloc(factory)
    req = encode_request_with_target(Kobako::Wire::Handle.new(parent_id), "make", [], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.ok?
    assert_kind_of Kobako::Wire::Handle, resp.payload
    refute_equal parent_id, resp.payload.id
    assert_equal "leaf", @handle_table.fetch(resp.payload.id).kind
  end

  def test_unknown_handle_target_returns_undefined_exception
    req = encode_request_with_target(Kobako::Wire::Handle.new(7), "any", [], {})

    resp = decode_response(@dispatcher.call(req))

    assert resp.err?
    assert_equal "undefined", resp.payload.type
  end

  # ---------- Cross-run invalidity (B-19 via HandleTable#reset!) ----------

  def test_handle_invalid_after_table_reset
    obj = Object.new
    def obj.tag = "t"
    handle_id = @handle_table.alloc(obj)
    @handle_table.reset!

    req = encode_request_with_target(Kobako::Wire::Handle.new(handle_id), "tag", [], {})
    resp = decode_response(@dispatcher.call(req))

    assert resp.err?
    assert_equal "undefined", resp.payload.type
  end

  private

  def encode_request_with_target(target, method, args, kwargs)
    Kobako::Wire::Envelope.encode_request(
      Kobako::Wire::Envelope::Request.new(target: target, method: method, args: args, kwargs: kwargs)
    )
  end

  def encode_request(target, method, args, kwargs)
    Kobako::Wire::Envelope.encode_request(target, method, args, kwargs)
  end

  def decode_response(bytes)
    Kobako::Wire::Envelope.decode_response(bytes)
  end
end
