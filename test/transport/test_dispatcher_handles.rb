# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of Handle traffic through Transport::Dispatcher:
# stateful return values wrapped as Handles (B-14) and Handle arguments /
# targets resolved against Catalog::Handles (B-16 / B-17). Per-run and
# per-Sandbox invalidity lives in test_dispatcher_invalidity.rb; wire
# violations in test_dispatcher_violations.rb.
class TestTransportDispatchHandles < Minitest::Test
  include DispatcherHelpers

  # ---------- B-14 — host wraps stateful return values as Handles ----------

  # SPEC B-14: a Service method whose return value falls outside the wire
  # type set (B-13) is automatically allocated a Catalog::Handles entry, and
  # the guest sees a Kobako::Handle in the Response.ok payload.
  def test_non_wire_return_value_is_wrapped_as_handle
    @registry.define(:Factory).bind(:Make, ->(name) { greeter(name) })
    req = encode_request("Factory::Make", "call", ["Alice"], {})

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_kind_of Kobako::Handle, resp.payload
    bound = @handler.fetch(resp.payload.id)
    assert_equal "hi,Alice", bound.greet
  end

  def test_primitive_return_value_is_not_wrapped
    @registry.define(:Logger).bind(:Echo, ->(arg) { arg })
    req = encode_request("Logger::Echo", "call", ["plain"], {})

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_equal "plain", resp.payload
    assert_equal 0, @handler.size
  end

  # ---------- B-16 — guest passes Handle as argument ----------

  # SPEC B-16: a Kobako::Handle arriving as an argument is resolved against
  # the Catalog::Handles before dispatch, and the bound Service method receives
  # the live Ruby object.
  def test_handle_arg_is_resolved_to_bound_object_before_dispatch
    greeter = Class.new do
      def initialize(name) = (@name = name)
      def greet = "hello,#{@name}"
    end.new("Alice")
    handle_id = alloc_id(greeter)
    @registry.define(:Echo).bind(:Wrap, ->(g) { "wrapped:#{g.greet}" })
    req = encode_request("Echo::Wrap", "call", [Kobako::Handle.restore(handle_id)], {})

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_equal "wrapped:hello,Alice", resp.payload
  end

  def test_handle_kwarg_is_resolved_to_bound_object_before_dispatch
    obj = Object.new
    def obj.greet = "kw_ok"
    handle_id = alloc_id(obj)
    capture = []
    @registry.define(:K).bind(:Run, target_kwarg_runner(capture))
    req = encode_request("K::Run", "run", [], { target: Kobako::Handle.restore(handle_id) })

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_equal "done", resp.payload
    assert_equal ["kw_ok"], capture
  end

  def test_unknown_handle_arg_returns_undefined_exception
    req = encode_request("Logger::Echo", "call", [Kobako::Handle.restore(999)], {})
    @registry.define(:Logger).bind(:Echo, ->(x) { x })

    resp = decode_response(dispatch(req))

    assert_predicate resp, :error?
    assert_equal "undefined", resp.payload.type
  end

  # ---------- B-17 — guest passes Handle as target (chained composition) -

  # SPEC B-17: a Kobako::Handle target resolves to the bound object directly;
  # the Server is bypassed and dispatch goes straight to public_send.
  def test_handle_target_is_dispatched_to_bound_object
    obj = Class.new do
      def find(id) = "row:#{id}"
    end.new
    handle_id = alloc_id(obj)

    resp = dispatch_handle_target(handle_id, "find", [42])

    assert_predicate resp, :ok?
    assert_equal "row:42", resp.payload
  end

  def test_handle_target_returning_stateful_value_is_wrapped_as_new_handle
    # B-17 + B-14 chained: invoking a Handle target whose method returns
    # another non-primitive object yields a fresh Handle in the response.
    parent_id = alloc_id(leaf_factory)

    resp = dispatch_handle_target(parent_id, "make")

    assert_predicate resp, :ok?
    assert_kind_of Kobako::Handle, resp.payload
    refute_equal parent_id, resp.payload.id
    assert_equal "leaf", @handler.fetch(resp.payload.id).kind
  end

  def test_unknown_handle_target_returns_undefined_exception
    resp = dispatch_handle_target(7, "any")

    assert_predicate resp, :error?
    assert_equal "undefined", resp.payload.type
  end

  private

  # Fixture: service member with `run(target:)` typed kwarg that pushes
  # `target.greet` into +capture+ and returns "done".
  def target_kwarg_runner(capture)
    klass = Class.new
    klass.define_method(:run) do |target:|
      capture << target.greet
      "done"
    end
    klass.new
  end

  # Fixture: stateful object with `name` + `greet(prefix="hi")` —
  # representative non-wire-representable return value (B-14).
  def greeter(name)
    Class.new do
      def initialize(name) = (@name = name)
      def greet(prefix = "hi") = "#{prefix},#{@name}"
    end.new(name)
  end

  # Fixture: factory whose `make` returns a fresh +leaf+ (each with
  # `kind = "leaf"`) — used to exercise B-14 + B-17 chained wrapping.
  def leaf_factory
    leaf = Class.new { def kind = "leaf" }
    Class.new { define_method(:make) { leaf.new } }.new
  end
end
