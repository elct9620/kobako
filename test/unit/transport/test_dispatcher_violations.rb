# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of Transport::Dispatcher containment: malformed wire
# payloads (non-Symbol kwargs keys, forged integer targets per B-20,
# over-deep nesting) come back as Response.error — never a host crash —
# and Catalog::Handles exhaustion (B-21 / E-07) surfaces through the same
# rescue chain. Well-formed dispatch lives in test_dispatcher.rb.
class TestTransportDispatchViolations < Minitest::Test
  include DispatcherHelpers

  # SPEC Wire Codec → Ext Types → ext 0x00: kwargs map keys MUST be ext
  # 0x00 Symbols. A non-Symbol key (String and Integer cover the natively
  # msgpack-representable shapes) decodes to a structurally valid
  # 5-element envelope, then fails the Request value object's kwargs-key
  # invariant; because that invariant is checked inside Request.decode's
  # block, Codec::Decoder.decode rescues the ArgumentError and re-raises
  # it as a wire-decode InvalidType, so the dispatcher reports
  # type="runtime". The envelope MUST carry all five elements: a
  # 4-element array would trip the arity guard first and never reach the
  # kwargs-key check — the second message assertion witnesses that the
  # kwargs-key path, not the arity guard, produced this error.
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
      resp = decode_response(dispatch(payload_call(Kobako::Codec::Encoder.encode([[], kwargs]))))

      assert_predicate resp, :error?
      assert_equal "runtime", resp.payload.type
      assert_match(/Sandbox received a malformed request/, resp.payload.message)
      assert_match(/kwargs keys must be Symbol/, resp.payload.message,
                   "#{shape} must be rejected by the kwargs-key invariant, not the arity guard")
    end
  end

  # ---------- Raw-int Handle rejection (SPEC B-20) ----------

  # SPEC B-20: a guest cannot forge a Capability Handle from a bare
  # integer. The host-side wire decoder rejects the malformed encoding
  # before the value reaches the Catalog::Handles. Operationally, a Request
  # whose target slot carries a raw msgpack int (no ext 0x01 framing)
  # fails Request.decode's type validation and the dispatcher
  # surfaces it as a Response.error. The integer never reaches resolve_target
  # or Catalog::Handles#fetch — see the assertion on table size below.
  #
  # The test seam: we cannot construct such a Request via Request.new
  # (its constructor rejects non-String/Handle target types). We hand-roll
  # the msgpack bytes via Kobako::Codec::Encoder so the malformed payload reaches
  # the dispatcher exactly as a misbehaving guest would emit it.
  def test_an_id_the_table_never_issued_is_refused_as_undefined
    resp = decode_response(dispatch(DispatcherHelpers.call_for(42, "call", ["x"])))

    assert_predicate resp, :error?
    # The core envelope makes a Handle target an ordinary integer the
    # guest picks, so the table's membership — not the wire shape — is
    # what refuses it (B-65). The guest observes a normal transport
    # error rather than a wasm trap.
    assert_equal "undefined", resp.payload.type,
                 "an integer through the Call target slot that the table never issued must be " \
                 "refused as an undefined target"
    assert_equal 0, @handler.size,
                 "a refused Handle id must not enter the Catalog::Handles"
  end

  # ---------- Over-deep wire violation (docs/wire-codec.md § Structural Nesting Depth) ----------

  # A guest request nested beyond the codec's depth bound must come back as a
  # Response.error with type="runtime" — the same containment as any other
  # malformed request, never a host crash or a wasm trap. The dispatcher
  # rescues only StandardError; this holds because the codec maps the nesting
  # overflow into the Kobako::Codec::Error taxonomy before it can become a
  # Ruby SystemStackError that would escape the rescue.
  def test_over_deep_request_is_contained_as_runtime_error
    # 1000 nested single-element arrays terminated by nil — a misbehaving
    # guest emitting a request far past the ecosystem nesting bound.
    over_deep_payload = "\x92#{"\x91" * 1000}\xC0\x80".b

    resp = decode_response(dispatch(payload_call(over_deep_payload)))

    assert_predicate resp, :error?
    assert_equal "runtime", resp.payload.type
    assert_match(/Sandbox received a malformed request/, resp.payload.message)
  end

  # ---------- Catalog::Handles exhaustion (SPEC B-21 / E-07) ----------

  # SPEC B-21 / E-07: when the per-#run Catalog::Handles counter reaches
  # MAX_ID (0x7fff_ffff), the next allocation must fail fast with
  # Kobako::HandleExhaustedError (a SandboxError subclass). The
  # dispatcher's wrap_return path is the call site that triggers this
  # during a normal transport call: a Service method returns a non-wire-representable
  # value, the codec raises UnsupportedType, wrap_return falls through to
  # @handler.alloc, and the cap raise surfaces via the dispatcher's
  # rescue chain as a Response.error the guest observes.
  def test_handler_exhaustion_during_wrap_return_is_response_err
    # Test seam: Catalog::Handles.new(next_id:) lets us pin the counter
    # at MAX_ID + 1 without 2^31 allocations. SPEC documents this seam
    # at Catalog::Handles "Build a fresh, empty Handler" — the parameter
    # is explicitly intended for cap-exhaustion testing.
    exhausted = Kobako::Catalog::Handles.new(next_id: Kobako::Handle::MAX_ID + 1)
    registry = Kobako::Catalog::Services.new
    registry.bind("Factory::Make", object_factory)
    req = encode_request("Factory::Make", "make", [], {})

    resp = decode_response(dispatch(req, server: registry, handler: exhausted))

    assert_predicate resp, :error?
    assert_equal "runtime", resp.payload.type
    assert_match(/Kobako::HandleExhaustedError/, resp.payload.message)
  end

  def test_handler_exhaustion_propagates_as_sandbox_error_class
    # Pin the class hierarchy: HandleExhaustedError < SandboxError
    # (per Kobako::errors). This matters because Sandbox-invocation-
    # level callers rescuing SandboxError must catch the exhaustion path;
    # the dispatcher's rescue StandardError branch turns the raise into
    # a Response.error so the guest can observe it, but the underlying
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

  # The dispatcher folds a Service's StandardError into a Response.error so
  # the guest can rescue it, but the boundary is StandardError by intent: a
  # host-process-level fault (here SecurityError, a non-StandardError) must
  # escape dispatch to trap the invocation rather than be masked as a
  # rescuable fault — the complement of the containment cases above.
  def test_non_standard_error_from_a_service_escapes_the_rescue
    @registry.bind("Boom::Fatal", ->(_) { raise SecurityError, "host fault" })
    req = encode_request("Boom::Fatal", "call", ["x"], {})

    error = assert_raises(SecurityError) { dispatch(req) }
    assert_equal "host fault", error.message,
                 "a non-StandardError raised by a Service must escape the dispatch rescue " \
                 "to trap the invocation, not fold into a guest-rescuable Response.error"
  end

  private

  # Fixture: factory whose `make` always returns a fresh Object — the
  # non-wire-representable return value that drives B-21 exhaustion.
  def object_factory
    Class.new { def make = Object.new }.new
  end
end
