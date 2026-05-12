# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of Registry#dispatch — fast and deterministic,
# exercises the Registry/Wire integration directly without a live
# Sandbox. Path-target dispatch, kwargs symbolization, Handle target /
# argument resolution, disconnected sentinel, cross-Sandbox invalidity,
# and HandleTable exhaustion are all observable through this seam.
# Live-Sandbox elevation of these paths lives in
# +test/test_e2e_journeys.rb+ via real mruby.
class TestRegistryDispatchUnit < Minitest::Test
  def setup
    @registry = Kobako::Registry.new
    @handle_table = @registry.handle_table
  end

  def test_dispatches_string_target_and_returns_response_ok_bytes
    @registry.define(:Logger).bind(:Echo, lambda(&:upcase))
    req = encode_request("Logger::Echo", "call", ["hi"], {})

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_equal "HI", resp.payload
  end

  def test_passes_kwargs_as_symbols_to_bound_object
    capture = []
    @registry.define(:Logger).bind(:Tag, kwarg_tag_recorder(capture))
    req = encode_request("Logger::Tag", "tag", ["x"], { "key" => "value" })

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_equal [%w[x value]], capture
  end

  def test_unknown_target_returns_undefined_exception
    req = encode_request("Missing::Method", "call", ["x"], {})

    resp = decode_response(@registry.dispatch(req))

    assert resp.err?
    assert_equal "undefined", resp.payload.type
  end

  def test_method_raise_returns_runtime_exception
    @registry.define(:Boom).bind(:Bang, ->(_) { raise "boom" })
    req = encode_request("Boom::Bang", "call", ["x"], {})

    resp = decode_response(@registry.dispatch(req))

    assert resp.err?
    assert_equal "runtime", resp.payload.type
    assert_match(/boom/, resp.payload.message)
  end

  def test_argument_error_returns_argument_exception
    @registry.define(:Service).bind(:M, ->(_a, _b) { :ok })
    # Missing argument — Ruby ArgumentError on dispatch.
    req = encode_request("Service::M", "call", [], {})

    resp = decode_response(@registry.dispatch(req))

    assert resp.err?
    assert_equal "argument", resp.payload.type
  end

  # ---------- E-15 — kwargs dispatch (Testing Layer 4) -------------------

  # SPEC E-15 (line 534) + Wire Contract Request kwargs (line 637) +
  # str/bin Encoding Rules (line 768). The dispatcher symbolizes string
  # keys before public_send to align with Ruby keyword-arg conventions.

  # SPEC line 815: empty kwargs is encoded as empty map `0x80`, never
  # absent. Methods whose signature accepts no keyword arguments must
  # still dispatch successfully when the wire carries an empty kwargs
  # map — the empty map is the wire-uniform shape for "no kwargs".
  def test_empty_kwargs_dispatches_to_no_kwarg_method
    @registry.define(:Math).bind(:Add, ->(a, b) { a + b })
    req = encode_request("Math::Add", "call", [2, 3], {})

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_equal 5, resp.payload
  end

  # SPEC E-15 explicit: "Passing keyword arguments to a method whose
  # signature accepts no keyword arguments is treated as a parameter
  # binding failure (type=\"argument\", E-15), not a Ruby runtime
  # exception (E-11)."
  def test_kwargs_to_no_kwarg_method_returns_argument_exception
    @registry.define(:Math).bind(:Add, ->(a, b) { a + b })
    req = encode_request("Math::Add", "call", [2, 3], { "extra" => 1 })

    resp = decode_response(@registry.dispatch(req))

    assert resp.err?
    assert_equal "argument", resp.payload.type
  end

  # SPEC E-15 explicit example: "unknown keyword" → type="argument".
  def test_unknown_keyword_returns_argument_exception
    klass = Class.new do
      define_method(:greet) { |name:| "hi,#{name}" }
    end
    @registry.define(:Hello).bind(:Greet, klass.new)
    req = encode_request("Hello::Greet", "greet", [], { "name" => "alice", "bogus" => "x" })

    resp = decode_response(@registry.dispatch(req))

    assert resp.err?
    assert_equal "argument", resp.payload.type
  end

  # SPEC line 768: kwargs map keys received as bin (ASCII-8BIT-encoded
  # String on the host) are decoded as UTF-8 strings and treated as
  # symbol-equivalent identifiers. The dispatcher must convert such
  # keys to symbols whose name matches the method's keyword parameter.
  def test_bin_encoded_kwargs_key_is_symbolized_for_dispatch
    klass = Class.new do
      define_method(:greet) { |name:| "hi,#{name}" }
    end
    @registry.define(:Hello).bind(:Greet, klass.new)
    bin_key = "name".dup.force_encoding(Encoding::ASCII_8BIT)
    # Construct a Request with a bin-encoded key bypassing the envelope's
    # Hash typing — the wire decoder produces ASCII-8BIT strings for bin
    # family payloads, which is the shape the dispatcher must accept.
    req = encode_request("Hello::Greet", "greet", [], { bin_key => "alice" })

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_equal "hi,alice", resp.payload
  end

  # SPEC line 760: kwargs map keys are str or bin (UTF-8 validated).
  # Non-string keys are a wire violation; the envelope decoder rejects
  # them and the dispatcher surfaces a wire-decode error.
  def test_non_string_kwargs_key_is_wire_violation
    bad_request_bytes = Kobako::Wire::Codec::Encoder.encode(
      ["Logger::Echo", "call", [], { 42 => "v" }]
    )

    resp = decode_response(@registry.dispatch(bad_request_bytes))

    assert resp.err?
    assert_equal "runtime", resp.payload.type
    assert_match(/wire decode failed/, resp.payload.message)
  end

  # Mixed positional + kwargs: the dispatcher passes positional args
  # first, then symbolized kwargs.
  def test_mixed_positional_and_kwargs_dispatches_correctly
    klass = Class.new do
      define_method(:set) { |key, value:| "#{key}=#{value}" }
    end
    @registry.define(:KV).bind(:Set, klass.new)
    req = encode_request("KV::Set", "set", ["k"], { "value" => "v" })

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_equal "k=v", resp.payload
  end

  # Method with **rest accepts any keys; the dispatcher symbolizes all
  # keys before splatting.
  def test_keyrest_method_accepts_arbitrary_kwargs
    obj = keyrest_recorder
    @registry.define(:K).bind(:Cap, obj)
    req = encode_request("K::Cap", "capture", [], { "a" => 1, "b" => 2 })

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_equal "ok", resp.payload
    assert_equal({ a: 1, b: 2 }, obj.captured)
  end

  # ---------- B-14 — host wraps stateful return values as Handles ----------

  # SPEC B-14: a Service method whose return value falls outside the wire
  # type set (B-13) is automatically allocated a HandleTable entry, and
  # the guest sees a Wire::Handle in the Response.ok payload.
  def test_non_wire_return_value_is_wrapped_as_handle
    @registry.define(:Factory).bind(:Make, ->(name) { greeter(name) })
    req = encode_request("Factory::Make", "call", ["Alice"], {})

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_kind_of Kobako::Wire::Handle, resp.payload
    bound = @handle_table.fetch(resp.payload.id)
    assert_equal "hi,Alice", bound.greet
  end

  def test_primitive_return_value_is_not_wrapped
    @registry.define(:Logger).bind(:Echo, ->(arg) { arg })
    req = encode_request("Logger::Echo", "call", ["plain"], {})

    resp = decode_response(@registry.dispatch(req))

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

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_equal "wrapped:hello,Alice", resp.payload
  end

  def test_handle_kwarg_is_resolved_to_bound_object_before_dispatch
    obj = Object.new
    def obj.greet = "kw_ok"
    handle_id = @handle_table.alloc(obj)
    capture = []
    @registry.define(:K).bind(:Run, target_kwarg_runner(capture))
    req = encode_request("K::Run", "run", [], { "target" => Kobako::Wire::Handle.new(handle_id) })

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_equal "done", resp.payload
    assert_equal ["kw_ok"], capture
  end

  def test_unknown_handle_arg_returns_undefined_exception
    req = encode_request("Logger::Echo", "call", [Kobako::Wire::Handle.new(999)], {})
    @registry.define(:Logger).bind(:Echo, ->(x) { x })

    resp = decode_response(@registry.dispatch(req))

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

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_equal "row:42", resp.payload
  end

  def test_handle_target_returning_stateful_value_is_wrapped_as_new_handle
    # B-17 + B-14 chained: invoking a Handle target whose method returns
    # another non-primitive object yields a fresh Handle in the response.
    parent_id = @handle_table.alloc(leaf_factory)
    req = encode_request_with_target(Kobako::Wire::Handle.new(parent_id), "make", [], {})

    resp = decode_response(@registry.dispatch(req))

    assert resp.ok?
    assert_kind_of Kobako::Wire::Handle, resp.payload
    refute_equal parent_id, resp.payload.id
    assert_equal "leaf", @handle_table.fetch(resp.payload.id).kind
  end

  def test_unknown_handle_target_returns_undefined_exception
    req = encode_request_with_target(Kobako::Wire::Handle.new(7), "any", [], {})

    resp = decode_response(@registry.dispatch(req))

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
    resp = decode_response(@registry.dispatch(req))

    assert resp.err?
    assert_equal "undefined", resp.payload.type
  end

  # ---------- Disconnected sentinel (SPEC E-14) ----------

  # SPEC E-14: a Handle whose entry has been replaced with the
  # +:disconnected+ sentinel (B-19 ABA protection) resolves to
  # Response.err(type="disconnected") at dispatch time, even though the id
  # still occupies the HandleTable. This is distinct from E-13 (unknown id
  # → "undefined"); the dispatcher must differentiate so the host can
  # surface +Kobako::ServiceError::Disconnected+ rather than a generic
  # ServiceError.
  def test_disconnected_handle_target_returns_disconnected_exception
    obj = Object.new
    def obj.any = "alive"
    handle_id = @handle_table.alloc(obj)
    @handle_table.mark_disconnected(handle_id)

    req = encode_request_with_target(Kobako::Wire::Handle.new(handle_id), "any", [], {})
    resp = decode_response(@registry.dispatch(req))

    assert resp.err?
    assert_equal "disconnected", resp.payload.type
  end

  # ---------- Cross-Sandbox-instance invalidity (SPEC B-19) ----------

  # SPEC B-19: HandleTable ownership is per-Sandbox. A Handle ID issued
  # by Sandbox A's HandleTable has no meaning in Sandbox B's HandleTable;
  # presenting it there resolves to "ID not found" and surfaces as a
  # Response.err with type="undefined". This is distinct from B-18
  # (cross-#run within the same Sandbox via #reset!): here we exercise
  # two physically separate HandleTable instances backing two separate
  # dispatchers, mirroring two live Sandbox instances.
  def test_handle_from_sandbox_a_is_undefined_in_sandbox_b_as_target
    registry_a = Kobako::Registry.new
    registry_b = Kobako::Registry.new
    handle_id_in_a = registry_a.handle_table.alloc(pinger)

    # Sanity: the integer id has meaning in A.
    assert_equal "pong", registry_a.handle_table.fetch(handle_id_in_a).ping

    # The integer id presented as a Handle target against B's registry
    # must NOT cross over: B's HandleTable does not contain that id.
    req = encode_request_with_target(Kobako::Wire::Handle.new(handle_id_in_a), "ping", [], {})
    resp = decode_response(registry_b.dispatch(req))

    assert resp.err?
    assert_equal "undefined", resp.payload.type
    assert_equal 0, registry_b.handle_table.size
  end

  def test_handle_from_sandbox_a_is_undefined_in_sandbox_b_as_arg
    # Same B-19 boundary, but the cross-Sandbox handle arrives as a
    # positional arg rather than the target. The Registry path resolves;
    # arg resolution fails when the id misses B's HandleTable.
    registry_a = Kobako::Registry.new
    registry_b = Kobako::Registry.new
    registry_b.define(:Echo).bind(:Wrap, ->(g) { "wrapped:#{g}" })
    table_a = registry_a.handle_table

    obj = Object.new
    handle_id_in_a = table_a.alloc(obj)

    req = encode_request("Echo::Wrap", "call", [Kobako::Wire::Handle.new(handle_id_in_a)], {})
    resp = decode_response(registry_b.dispatch(req))

    assert resp.err?
    assert_equal "undefined", resp.payload.type
  end

  # ---------- Raw-int Handle rejection (SPEC B-20) ----------

  # SPEC B-20: a guest cannot forge a Capability Handle from a bare
  # integer. The host-side wire decoder rejects the malformed encoding
  # before the value reaches the HandleTable. Operationally, a Request
  # whose target slot carries a raw msgpack int (no ext 0x01 framing)
  # fails Envelope.decode_request's type validation and the dispatcher
  # surfaces it as a Response.err. The integer never reaches resolve_target
  # or HandleTable#fetch — see the assertion on table size below.
  #
  # The test seam: we cannot construct such a Request via Request.new
  # (its constructor rejects non-String/Handle target types). We hand-roll
  # the msgpack bytes via Wire::Codec::Encoder so the malformed payload reaches
  # the dispatcher exactly as a misbehaving guest would emit it.
  def test_raw_integer_target_is_rejected_by_wire_decoder_as_violation
    bad_request_bytes = Kobako::Wire::Codec::Encoder.encode([42, "call", ["x"], {}])

    resp = decode_response(@registry.dispatch(bad_request_bytes))

    assert resp.err?
    # Wire::Codec::Error rescues to type="runtime" with a "wire decode failed"
    # prefix; the dispatcher's contract pins this taxonomy and the guest
    # observes a normal RPC error rather than a wasm trap.
    assert_equal "runtime", resp.payload.type
    assert_match(/wire decode failed/, resp.payload.message)
    # The malformed int never made it into the HandleTable.
    assert_equal 0, @handle_table.size
  end

  # ---------- HandleTable exhaustion (SPEC B-21 / E-07) ----------

  # SPEC B-21 / E-07: when the per-#run HandleTable counter reaches
  # MAX_ID (0x7fff_ffff), the next allocation must fail fast with
  # Kobako::HandleTableExhausted (a SandboxError subclass). The
  # dispatcher's wrap_return path is the call site that triggers this
  # during normal RPC: a Service method returns a non-wire-representable
  # value, the codec raises UnsupportedType, wrap_return falls through to
  # @handle_table.alloc, and the cap raise surfaces via the dispatcher's
  # rescue chain as a Response.err the guest observes.
  def test_handle_table_exhaustion_during_wrap_return_is_response_err
    # Test seam: HandleTable.new(next_id:) lets us pin the counter at
    # MAX_ID + 1 without 2^31 allocations. SPEC documents this seam at
    # HandleTable "Build a fresh, empty HandleTable" — the parameter is
    # explicitly intended for cap-exhaustion testing.
    registry = registry_with_exhausted_handle_table
    registry.define(:Factory).bind(:Make, object_factory)
    req = encode_request("Factory::Make", "make", [], {})

    resp = decode_response(registry.dispatch(req))

    assert resp.err?
    assert_equal "runtime", resp.payload.type
    assert_match(/Kobako::HandleTableExhausted/, resp.payload.message)
  end

  def test_handle_table_exhaustion_propagates_as_sandbox_error_class
    # Pin the class hierarchy: HandleTableExhausted < HandleTableError <
    # SandboxError (per Kobako::errors). This matters because
    # Sandbox#run-level callers rescuing SandboxError must catch the
    # exhaustion path; the dispatcher's rescue StandardError branch
    # turns the raise into a Response.err so the guest can observe it,
    # but the underlying class identity is what SPEC B-21 pins.
    assert_operator Kobako::HandleTableExhausted, :<, Kobako::HandleTableError
    assert_operator Kobako::HandleTableError, :<, Kobako::SandboxError

    table = Kobako::Registry::HandleTable.new(
      next_id: Kobako::Registry::HandleTable::MAX_ID + 1
    )
    error = assert_raises(Kobako::SandboxError) do
      table.alloc(Object.new)
    end
    assert_kind_of Kobako::HandleTableExhausted, error
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

  # Fixture: object with a single `ping → "pong"` method, the minimum
  # Handle target needed for cross-Sandbox B-19 invalidity coverage.
  def pinger
    obj = Object.new
    def obj.ping = "pong"
    obj
  end

  # Fixture: factory whose `make` always returns a fresh Object — the
  # non-wire-representable return value that drives B-21 exhaustion.
  def object_factory
    Class.new { def make = Object.new }.new
  end

  # Build a Registry whose HandleTable counter is pinned at MAX_ID + 1
  # so the next #alloc trips the B-21 cap.
  def registry_with_exhausted_handle_table
    registry = Kobako::Registry.new
    exhausted = Kobako::Registry::HandleTable.new(next_id: Kobako::Registry::HandleTable::MAX_ID + 1)
    registry.instance_variable_set(:@handle_table, exhausted)
    registry
  end
end
