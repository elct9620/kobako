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
  # the guest sees a Kobako::Handle on the Reply's ok arm.
  # @behavior T-001
  def test_non_wire_return_value_is_wrapped_as_handle
    @registry.bind("Factory::Make", ->(name) { greeter(name) })
    call = build_call("Factory::Make", "call", ["Alice"], {})

    answer = reify(dispatch(call))

    assert_predicate answer, :ok?
    assert_kind_of Kobako::Handle, answer.payload
    bound = @handler.fetch(answer.payload.id)
    assert_equal "hi,Alice", bound.greet
  end

  # @behavior T-002
  def test_primitive_return_value_is_not_wrapped
    @registry.bind("Logger::Echo", ->(arg) { arg })
    call = build_call("Logger::Echo", "call", ["plain"], {})

    answer = reify(dispatch(call))

    assert_predicate answer, :ok?
    assert_equal "plain", answer.payload
    assert_equal 0, @handler.size
  end

  # ---------- B-16 — guest passes Handle as argument ----------

  # SPEC B-16: a Kobako::Handle arriving as an argument is resolved against
  # the Catalog::Handles before dispatch, and the bound Service method receives
  # the live Ruby object.
  # @behavior T-003
  def test_handle_arg_is_resolved_to_bound_object_before_dispatch
    greeter = Class.new do
      def initialize(name) = (@name = name)
      def greet = "hello,#{@name}"
    end.new("Alice")
    handle_id = alloc_id(greeter)
    @registry.bind("Echo::Wrap", ->(g) { "wrapped:#{g.greet}" })
    call = build_call("Echo::Wrap", "call", [Kobako::Handle.restore(handle_id)], {})

    answer = reify(dispatch(call))

    assert_predicate answer, :ok?
    assert_equal "wrapped:hello,Alice", answer.payload
  end

  # @behavior T-004
  def test_handle_kwarg_is_resolved_to_bound_object_before_dispatch
    obj = Object.new
    def obj.greet = "kw_ok"
    handle_id = alloc_id(obj)
    capture = []
    @registry.bind("K::Run", target_kwarg_runner(capture))
    call = build_call("K::Run", "run", [], { target: Kobako::Handle.restore(handle_id) })

    answer = reify(dispatch(call))

    assert_predicate answer, :ok?
    assert_equal "done", answer.payload
    assert_equal ["kw_ok"], capture
  end

  # @behavior T-005
  def test_unknown_handle_arg_returns_undefined_exception
    call = build_call("Logger::Echo", "call", [Kobako::Handle.restore(999)], {})
    @registry.bind("Logger::Echo", ->(x) { x })

    answer = reify(dispatch(call))

    assert_predicate answer, :error?
    assert_equal "undefined", answer.payload.type
  end

  # ---------- B-17 — guest passes Handle as target (chained composition) -

  # SPEC B-17: a Kobako::Handle target resolves to the bound object directly;
  # the Server is bypassed and dispatch goes straight to public_send.
  # @behavior T-006
  def test_handle_target_is_dispatched_to_bound_object
    obj = Class.new do
      def find(id) = "row:#{id}"
    end.new
    handle_id = alloc_id(obj)

    answer = dispatch_handle_target(handle_id, "find", [42])

    assert_predicate answer, :ok?
    assert_equal "row:42", answer.payload
  end

  # @behavior T-007
  def test_handle_target_returning_stateful_value_is_wrapped_as_new_handle
    # B-17 + B-14 chained: invoking a Handle target whose method returns
    # another non-primitive object yields a fresh Handle on the ok arm.
    parent_id = alloc_id(leaf_factory)

    answer = dispatch_handle_target(parent_id, "make")

    assert_predicate answer, :ok?
    assert_kind_of Kobako::Handle, answer.payload
    refute_equal parent_id, answer.payload.id
    assert_equal "leaf", @handler.fetch(answer.payload.id).kind
  end

  # @behavior T-008
  def test_unknown_handle_target_returns_undefined_exception
    answer = dispatch_handle_target(7, "any")

    assert_predicate answer, :error?
    assert_equal "undefined", answer.payload.type
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
