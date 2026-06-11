# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of Transport::Dispatcher.dispatch — path-target
# dispatch, kwargs symbolization (E-15), and the error taxonomy for raised
# Service methods. Handle resolution lives in test_dispatcher_handles.rb;
# wire violations and exhaustion in test_dispatcher_violations.rb.
class TestTransportDispatchUnit < Minitest::Test
  include DispatcherHelpers

  def test_dispatches_string_target_and_returns_response_ok_bytes
    @registry.define(:Logger).bind(:Echo, lambda(&:upcase))
    req = encode_request("Logger::Echo", "call", ["hi"], {})

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_equal "HI", resp.payload
  end

  def test_passes_kwargs_as_symbols_to_bound_object
    capture = []
    @registry.define(:Logger).bind(:Tag, kwarg_tag_recorder(capture))
    req = encode_request("Logger::Tag", "tag", ["x"], { key: "value" })

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_equal [%w[x value]], capture
  end

  def test_unknown_target_returns_undefined_exception
    req = encode_request("Missing::Method", "call", ["x"], {})

    resp = decode_response(dispatch(req))

    assert_predicate resp, :error?
    assert_equal "undefined", resp.payload.type
  end

  def test_method_raise_returns_runtime_exception
    @registry.define(:Boom).bind(:Bang, ->(_) { raise "boom" })
    req = encode_request("Boom::Bang", "call", ["x"], {})

    resp = decode_response(dispatch(req))

    assert_predicate resp, :error?
    assert_equal "runtime", resp.payload.type
    assert_match(/boom/, resp.payload.message)
  end

  def test_argument_error_returns_argument_exception
    @registry.define(:Service).bind(:M, ->(_a, _b) { :ok })
    # Missing argument — Ruby ArgumentError on dispatch.
    req = encode_request("Service::M", "call", [], {})

    resp = decode_response(dispatch(req))

    assert_predicate resp, :error?
    assert_equal "argument", resp.payload.type
  end

  # ---------- E-15 — kwargs dispatch (Testing Layer 4) -------------------

  # SPEC E-15 + Wire Contract Request kwargs + Ext Types → ext 0x00.
  # Keyword argument names travel on the wire as Symbols; the dispatcher
  # forwards them to +public_send+ without further conversion.

  # SPEC: empty kwargs is encoded as empty map `0x80`, never absent.
  # Methods whose signature accepts no keyword arguments must still
  # dispatch successfully when the wire carries an empty kwargs map —
  # the empty map is the wire-uniform shape for "no kwargs".
  def test_empty_kwargs_dispatches_to_no_kwarg_method
    @registry.define(:Math).bind(:Add, ->(a, b) { a + b })
    req = encode_request("Math::Add", "call", [2, 3], {})

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_equal 5, resp.payload
  end

  # SPEC E-15 explicit: "Passing keyword arguments to a method whose
  # signature accepts no keyword arguments is treated as a parameter
  # binding failure (type=\"argument\", E-15), not a Ruby runtime
  # exception (E-11)."
  def test_kwargs_to_no_kwarg_method_returns_argument_exception
    @registry.define(:Math).bind(:Add, ->(a, b) { a + b })
    req = encode_request("Math::Add", "call", [2, 3], { extra: 1 })

    resp = decode_response(dispatch(req))

    assert_predicate resp, :error?
    assert_equal "argument", resp.payload.type
  end

  # SPEC E-15 explicit example: "unknown keyword" → type="argument".
  def test_unknown_keyword_returns_argument_exception
    klass = Class.new do
      define_method(:greet) { |name:| "hi,#{name}" }
    end
    @registry.define(:Hello).bind(:Greet, klass.new)
    req = encode_request("Hello::Greet", "greet", [], { name: "alice", bogus: "x" })

    resp = decode_response(dispatch(req))

    assert_predicate resp, :error?
    assert_equal "argument", resp.payload.type
  end

  # Mixed positional + kwargs: the dispatcher passes positional args
  # first, then the Symbol-keyed kwargs hash.
  def test_mixed_positional_and_kwargs_dispatches_correctly
    klass = Class.new do
      define_method(:set) { |key, value:| "#{key}=#{value}" }
    end
    @registry.define(:KV).bind(:Set, klass.new)
    req = encode_request("KV::Set", "set", ["k"], { value: "v" })

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_equal "k=v", resp.payload
  end

  # Method with **rest accepts any keys; the dispatcher forwards them
  # unchanged to public_send.
  def test_keyrest_method_accepts_arbitrary_kwargs
    obj = keyrest_recorder
    @registry.define(:K).bind(:Cap, obj)
    req = encode_request("K::Cap", "capture", [], { a: 1, b: 2 })

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_equal "ok", resp.payload
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
