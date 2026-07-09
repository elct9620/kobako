# frozen_string_literal: true

require "test_helper"

# The Fault envelope's positional legality on the transport paths (E-50):
# its sole legal wire position is the Response status=1 field, so a
# guest→host payload smuggling an ext 0x02 is rejected — otherwise a
# Handle nested in its details would reach host code as a token nothing
# can resolve. Outbound, the host never emits one in a payload position:
# a Fault return value takes the B-14 auto-wrap path and a Fault yield
# argument is refused at the yield site. The outcome-envelope paths of
# E-50 are pinned in test/outcome/test_attribution.rb; the shipped guest
# cannot emit ext 0x02, so all positions are pinned host-side with
# hand-crafted bytes.
class TestTransportFaultPosition < Minitest::Test
  include DispatcherHelpers

  FAULT = Kobako::Fault.new(type: "runtime", message: "smuggled")

  # ---------- E-50 — inbound Request path ----------

  def test_request_carrying_fault_in_args_is_rejected_as_malformed
    @registry.define(:Echo).bind(:Id, ->(x) { x })
    # Hand-crafted via the bare codec: the raw wire tool stays permissive,
    # the positional rule lives on the envelope decode.
    req = Kobako::Codec::Encoder.encode(["Echo::Id", "call", [FAULT], {}, false])

    resp = decode_response(dispatch(req))

    assert_predicate resp, :error?
    assert_equal "runtime", resp.payload.type,
                 "E-50: a Request carrying an ext 0x02 Fault must be rejected through the malformed-payload channel"
    assert_match(/malformed request/, resp.payload.message,
                 "the rejection must surface as the dispatcher's malformed-request fault")
  end

  # ---------- E-50 — inbound YieldResponse path ----------

  def test_yield_response_carrying_fault_raises_at_the_yield_site
    bytes = Kobako::Transport::Yield.new(tag: Kobako::Transport::TAG_OK, value: FAULT).encode
    yielder = Kobako::Transport::Yielder.new(->(_args) { bytes }, :__test_break__, @handler)

    assert_raises(Kobako::Codec::InvalidType,
                  "E-50: a YieldResponse ok value carrying an ext 0x02 Fault must raise at the Service yield site") do
      yielder.yield
    end
  end

  # ---------- outbound — Fault has no wire representation in payload positions ----------

  def test_fault_return_value_is_wrapped_as_handle
    @registry.define(:Errors).bind(:Last, -> { FAULT })
    req = encode_request("Errors::Last", "call", [], {})

    resp = decode_response(dispatch(req))

    assert_predicate resp, :ok?
    assert_kind_of Kobako::Handle, resp.payload,
                   "a Fault returned by a Service must take the B-14 auto-wrap path, never ride as ext 0x02"
    assert_same FAULT, @handler.fetch(resp.payload.id),
                "the Handle must resolve back to the original Fault object"
  end

  def test_fault_yield_argument_is_refused_at_the_yield_site
    yielder = Kobako::Transport::Yielder.new(->(_args) { flunk "guest must not be re-entered" }, :__t__, @handler)

    assert_raises(Kobako::Codec::UnsupportedType,
                  "a Fault yield argument has no wire representation in a payload position and must be refused") do
      yielder.yield(FAULT)
    end
  end

  # ---------- bracket hygiene — the forbid_faults analogue of the depth-residue guard ----------

  def test_rejected_payload_leaves_the_legal_position_usable_on_the_same_thread
    bad = Kobako::Codec::Encoder.encode(["Echo::Id", "call", [FAULT], {}, false])
    assert_raises(Kobako::Codec::InvalidType) { Kobako::Transport::Request.decode(bad) }

    legal = Kobako::Transport::Response.error(FAULT).encode
    decoded = Kobako::Transport::Response.decode(legal)
    assert_equal FAULT, decoded.payload,
                 "a rejected payload-position decode must leave no residue — the Response fault field " \
                 "on the same thread stays decodable"
  end
end
