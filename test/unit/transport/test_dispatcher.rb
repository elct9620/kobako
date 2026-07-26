# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of Transport::Dispatcher.dispatch — path-target
# dispatch, kwargs symbolization (E-15), and the error taxonomy for raised
# Service methods. Handle resolution lives in test_dispatcher_handles.rb;
# wire violations and exhaustion in test_dispatcher_violations.rb.
class TestTransportDispatchUnit < Minitest::Test
  include DispatcherHelpers

  def test_dispatches_string_target_and_returns_ok_arm_bytes
    @registry.bind("Logger::Echo", lambda(&:upcase))
    call = build_call("Logger::Echo", "call", ["hi"], {})

    answer = reify(dispatch(call))

    assert_predicate answer, :ok?
    assert_equal "HI", answer.payload
  end

  def test_passes_kwargs_as_symbols_to_bound_object
    capture = []
    @registry.bind("Logger::Tag", kwarg_tag_recorder(capture))
    call = build_call("Logger::Tag", "tag", ["x"], { key: "value" })

    answer = reify(dispatch(call))

    assert_predicate answer, :ok?
    assert_equal [%w[x value]], capture
  end

  # E-12: a dispatch target path matching no registered Service surfaces as
  # the type="undefined" fault; the dispatcher never raises.
  def test_unknown_target_returns_undefined_exception
    call = build_call("Missing::Method", "call", ["x"], {})

    answer = reify(dispatch(call))

    assert_predicate answer, :error?
    assert_equal "undefined", answer.payload.type
  end

  def test_method_raise_returns_runtime_exception
    @registry.bind("Boom::Bang", ->(_) { raise "boom" })
    call = build_call("Boom::Bang", "call", ["x"], {})

    answer = reify(dispatch(call))

    assert_predicate answer, :error?
    assert_equal "runtime", answer.payload.type
    assert_match(/boom/, answer.payload.message)
  end

  def test_argument_error_returns_argument_exception
    @registry.bind("Service::M", ->(_a, _b) { :ok })
    # Missing argument — Ruby ArgumentError on dispatch.
    call = build_call("Service::M", "call", [], {})

    answer = reify(dispatch(call))

    assert_predicate answer, :error?
    assert_equal "argument", answer.payload.type
  end

  # ---------- E-15 — kwargs dispatch (Testing Layer 4) -------------------

  # SPEC E-15 + Wire Contract Call kwargs + Ext Types → ext 0x00.
  # Keyword argument names travel on the wire as Symbols; the dispatcher
  # forwards them to +public_send+ without further conversion.

  # SPEC: empty kwargs is encoded as empty map `0x80`, never absent.
  # Methods whose signature accepts no keyword arguments must still
  # dispatch successfully when the wire carries an empty kwargs map —
  # the empty map is the wire-uniform shape for "no kwargs".
  def test_empty_kwargs_dispatches_to_no_kwarg_method
    @registry.bind("Math::Add", ->(a, b) { a + b })
    call = build_call("Math::Add", "call", [2, 3], {})

    answer = reify(dispatch(call))

    assert_predicate answer, :ok?
    assert_equal 5, answer.payload
  end

  # SPEC E-15 explicit: "Passing keyword arguments to a method whose
  # signature accepts no keyword arguments is treated as a parameter
  # binding failure (type=\"argument\", E-15), not a Ruby runtime
  # exception (E-11)." Ruby core builds the arity ArgumentError message
  # as ASCII-8BIT and the guest proxy refuses a bin message field, so
  # the message's decoded encoding (str decodes UTF-8, bin ASCII-8BIT)
  # is asserted alongside the fault type.
  def test_kwargs_to_no_kwarg_method_returns_argument_exception
    @registry.bind("Math::Add", ->(a, b) { a + b })
    call = build_call("Math::Add", "call", [2, 3], { extra: 1 })

    answer = reify(dispatch(call))

    assert_predicate answer, :error?
    assert_equal "argument", answer.payload.type
    assert_equal Encoding::UTF_8, answer.payload.message.encoding,
                 "a binding-failure fault through Dispatcher.dispatch must carry its message as a wire str, not bin"
  end

  # SPEC E-15 explicit example: "unknown keyword" → type="argument".
  def test_unknown_keyword_returns_argument_exception
    klass = Class.new do
      define_method(:greet) { |name:| "hi,#{name}" }
    end
    @registry.bind("Hello::Greet", klass.new)
    call = build_call("Hello::Greet", "greet", [], { name: "alice", bogus: "x" })

    answer = reify(dispatch(call))

    assert_predicate answer, :error?
    assert_equal "argument", answer.payload.type
  end

  # Mixed positional + kwargs: the dispatcher passes positional args
  # first, then the Symbol-keyed kwargs hash.
  def test_mixed_positional_and_kwargs_dispatches_correctly
    klass = Class.new do
      define_method(:set) { |key, value:| "#{key}=#{value}" }
    end
    @registry.bind("KV::Set", klass.new)
    call = build_call("KV::Set", "set", ["k"], { value: "v" })

    answer = reify(dispatch(call))

    assert_predicate answer, :ok?
    assert_equal "k=v", answer.payload
  end

  # Method with **rest accepts any keys; the dispatcher forwards them
  # unchanged to public_send.
  def test_keyrest_method_accepts_arbitrary_kwargs
    obj = keyrest_recorder
    @registry.bind("K::Cap", obj)
    call = build_call("K::Cap", "capture", [], { a: 1, b: 2 })

    answer = reify(dispatch(call))

    assert_predicate answer, :ok?
    assert_equal "ok", answer.payload
    assert_equal({ a: 1, b: 2 }, obj.captured)
  end

  private

  # Fixture: service member that records each `tag(arg, key:)` invocation
  # into +capture+ and returns "ok".
  def kwarg_tag_recorder(capture)
    klass = Class.new
    klass.define_method(:tag) do |arg, key:|
      capture << [arg, key]
      "ok"
    end
    klass.new
  end

  # Fixture: service member with `capture(**opts)` keyrest, stashing
  # opts into the returned object's `captured` reader.
  def keyrest_recorder
    Class.new do
      attr_reader :captured

      def capture(**opts)
        @captured = opts
        "ok"
      end
    end.new
  end
end
