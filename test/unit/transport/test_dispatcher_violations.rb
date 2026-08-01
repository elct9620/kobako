# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of Transport::Dispatcher containment: the
# malformed-payload channel (E-10 — non-Symbol kwargs keys, over-deep
# nesting), a Handle id the invocation never issued (B-65), and
# Catalog::Handles exhaustion (B-21 / E-07) all come back on the Reply's
# fault arm — never a host crash. Well-formed dispatch lives in
# test_dispatcher.rb.
class TestTransportDispatchViolations < Minitest::Test
  include DispatcherHelpers

  # E-10, on the ext 0x00 rule (docs/wire/payload-msgpack.md § Ext
  # Types → ext 0x00): kwargs map keys MUST be ext
  # 0x00 Symbols. A non-Symbol key (String and Integer cover the natively
  # msgpack-representable shapes) decodes to a structurally valid
  # 2-element payload, then fails the Payload::Arguments kwargs-key
  # invariant; because that invariant is checked inside the block
  # Arguments.decode yields to, Codec::Decoder.decode rescues the
  # ArgumentError and re-raises it as a wire-decode InvalidTypeError, so the
  # dispatcher reports type="internal" — the request never became a call,
  # so no Service outcome exists to report. The payload MUST carry both
  # elements: a 1-element array would trip the shape guard first and never
  # reach the kwargs-key check — the second message assertion witnesses
  # that the kwargs-key path, not the shape guard, produced this error.
  NON_SYMBOL_KWARGS = {
    "a String kwargs key" => { "name" => "alice" },
    "an Integer kwargs key" => { 42 => "v" }
  }.freeze

  # A Call at the bound echo path carrying +payload+ verbatim, so a test
  # can hand the payload decode a shape the value object would refuse to
  # build.
  def payload_call(payload)
    Kobako::Transport::Call.new(target: "Logger::Echo", method_name: "call",
                                block_given: false, payload: payload)
  end

  def test_non_symbol_kwargs_key_is_wire_violation
    NON_SYMBOL_KWARGS.each do |shape, kwargs|
      answer = reify(dispatch(payload_call(Kobako::Codec::Encoder.encode([[], kwargs]))))

      assert_predicate answer, :error?
      assert_equal "internal", answer.payload.type,
                   "#{shape} through the Call payload must refuse as an internal fault — the " \
                   "Service never ran, so nothing about it failed"
      assert_match(/Sandbox could not read the request/, answer.payload.message)
      assert_match(/kwargs keys must be Symbol/, answer.payload.message,
                   "#{shape} must be rejected by the kwargs-key invariant, not the arity guard")
    end
  end

  # ---------- Un-issued Handle id (SPEC B-65 / E-13) ----------

  # SPEC B-65: the core envelope carries a Handle target as a bare id its
  # sender chooses, so an id the table never issued is a well-formed Call
  # that reaches the host — the invocation's Catalog::Handles membership
  # is what refuses it, not the wire shape. The guest sees a transport
  # error on the fault arm rather than a wasm trap, and the refused id
  # leaves the table untouched, which the size assertion witnesses.
  def test_an_id_the_table_never_issued_is_refused_as_undefined
    answer = reify(dispatch(DispatcherHelpers.call_for(42, "call", ["x"])))

    assert_predicate answer, :error?
    assert_equal "undefined", answer.payload.type,
                 "an integer through the Call target slot that the table never issued must be " \
                 "refused as an undefined target"
    assert_equal 0, @handler.size,
                 "a refused Handle id must not enter the Catalog::Handles"
  end

  # ---------- Over-deep wire violation (E-10, on the depth bound) ----------

  # E-10: a Call nested beyond the codec's depth bound
  # (docs/wire/payload-msgpack.md § Structural Nesting Depth) must come back
  # on the fault arm with type="internal" — the same containment as any other
  # malformed payload, never a host crash or a wasm trap. The dispatcher
  # rescues only StandardError; this holds because the codec maps the nesting
  # overflow into the Kobako::Codec::Error taxonomy before it can become a
  # Ruby SystemStackError that would escape the rescue.
  def test_over_deep_call_is_contained_as_an_internal_fault
    # 1000 nested single-element arrays terminated by nil — a misbehaving
    # guest emitting a payload far past the ecosystem nesting bound.
    over_deep_payload = "\x92#{"\x91" * 1000}\xC0\x80".b

    answer = reify(dispatch(payload_call(over_deep_payload)))

    assert_predicate answer, :error?
    assert_equal "internal", answer.payload.type,
                 "a payload past the nesting bound through the Call payload must refuse as " \
                 "an internal fault — the Service never ran, so nothing about it failed"
    assert_match(/Sandbox could not read the request/, answer.payload.message)
  end

  # ---------- Catalog::Handles exhaustion (SPEC B-21 / E-07) ----------

  # SPEC B-21 / E-07: when the per-#run Catalog::Handles counter reaches
  # MAX_ID (0x7fff_ffff), the next allocation must fail fast with
  # Kobako::HandleExhaustedError (a SandboxError subclass). The
  # dispatcher's wrap_return path is the call site that triggers this
  # during a normal transport call: a Service method returns a non-wire-representable
  # value, the codec raises UnsupportedTypeError, wrap_return falls through to
  # @handler.alloc, and the cap raise surfaces via the dispatcher's
  # rescue chain on the fault arm the guest observes.
  def test_handler_exhaustion_during_wrap_return_takes_the_fault_arm
    answer = reify(dispatch(build_call("Factory::Make", "make", [], {}),
                            server: factory_registry, handler: exhausted_handles))

    assert_predicate answer, :error?
    assert_equal "internal", answer.payload.type,
                 "a Service answer the exhausted table cannot issue a Handle for must refuse " \
                 "as an internal fault — the Service ran and succeeded"
    assert_match(/Out of handle allocations/, answer.payload.message)
    refute_match(/Kobako::/, answer.payload.message,
                 "kobako's own refusal must not wear the <class>: <message> shape a Service " \
                 "exception crosses in, which would read as the Service having raised it")
  end

  def test_handler_exhaustion_propagates_as_sandbox_error_class
    # Pin the class hierarchy: HandleExhaustedError < SandboxError
    # (per Kobako::errors). This matters because Sandbox-invocation-
    # level callers rescuing SandboxError must catch the exhaustion path;
    # the dispatcher's rescue StandardError branch turns the raise into
    # a fault the guest can observe, but the underlying
    # class identity is what SPEC B-21 pins.
    assert_operator Kobako::HandleExhaustedError, :<, Kobako::SandboxError

    table = Kobako::Catalog::Handles.new(
      next_id: Kobako::Handle::MAX_ID + 1
    )
    error = assert_raises(Kobako::SandboxError) do
      table.alloc(Object.new)
    end
    assert_kind_of Kobako::HandleExhaustedError, error
  end

  # ---------- Host-level fault escapes the rescue by design ----------

  # The dispatcher folds a Service's StandardError onto the fault arm so
  # the guest can rescue it, but the boundary is StandardError by intent: a
  # host-process-level fault (here SecurityError, a non-StandardError) must
  # escape dispatch to trap the invocation rather than be masked as a
  # rescuable fault — the complement of the containment cases above.
  def test_non_standard_error_from_a_service_escapes_the_rescue
    @registry.bind("Boom::Fatal", ->(_) { raise SecurityError, "host fault" })
    call = build_call("Boom::Fatal", "call", ["x"], {})

    error = assert_raises(SecurityError) { dispatch(call) }
    assert_equal "host fault", error.message,
                 "a non-StandardError raised by a Service must escape the dispatch rescue " \
                 "to trap the invocation, not fold into a guest-rescuable fault"
  end

  private

  # Fixture: factory whose `make` always returns a fresh Object — the
  # non-wire-representable return value that drives B-21 exhaustion.
  def object_factory
    Class.new { def make = Object.new }.new
  end

  def factory_registry
    Kobako::Catalog::Services.new.tap { |registry| registry.bind("Factory::Make", object_factory) }
  end

  # Test seam: Catalog::Handles.new(next_id:) pins the counter at
  # MAX_ID + 1 without 2^31 allocations. Catalog::Handles documents the
  # parameter as intended for cap-exhaustion testing.
  def exhausted_handles
    Kobako::Catalog::Handles.new(next_id: Kobako::Handle::MAX_ID + 1)
  end
end
