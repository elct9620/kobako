# frozen_string_literal: true

require "test_helper"

# Unit + integration tests for the host-side Response envelope encoder /
# decoder (SPEC.md F-05 / F-09), mirroring lib/kobako/transport/response.rb,
# plus the cross-envelope Handle nesting round-trip. Builds on top of the
# byte-level wire codec covered by test/codec/; no native extension
# dependency. The Request side lives in test_request.rb.
class TestTransportResponse < Minitest::Test
  include CodecHelpers

  Envelope = Kobako::Transport

  def test_response_ok_round_trip_with_primitive
    resp = Envelope::Response.ok(42)
    decoded = Envelope::Response.decode(resp.encode)
    assert decoded.ok?
    assert_equal 42, decoded.payload
  end

  def test_response_ok_round_trip_with_handle
    resp = Envelope::Response.ok(Handle.restore(99))
    decoded = Envelope::Response.decode(resp.encode)
    assert decoded.ok?
    assert_instance_of Handle, decoded.payload
    assert_equal 99, decoded.payload.id
  end

  def test_response_error_round_trip
    exc = Exc.new(type: "runtime", message: "boom", details: nil)
    resp = Envelope::Response.error(exc)
    decoded = Envelope::Response.decode(resp.encode)
    assert decoded.error?
    assert_equal exc, decoded.payload
  end

  def test_response_error_requires_fault
    assert_raises(ArgumentError) { Envelope::Response.error("not a fault") }
  end

  def test_response_construction_validates_field_types
    assert_raises(ArgumentError) { Envelope::Response.new(status: 99,                        payload: nil) }
    assert_raises(ArgumentError) { Envelope::Response.new(status: -1,                        payload: nil) }
    assert_raises(ArgumentError) { Envelope::Response.new(status: Envelope::STATUS_ERROR, payload: "str") }
    assert_raises(ArgumentError) { Envelope::Response.new(status: Envelope::STATUS_ERROR, payload: 42) }
  end

  def test_response_decode_rejects_unknown_status
    bytes = Encoder.encode([2, nil])
    assert_raises(InvalidType) { Envelope::Response.decode(bytes) }
  end

  def test_response_decode_rejects_wrong_arity
    # 3-element array, not 2. Symmetric with
    # +test_request_decode_rejects_wrong_arity+ in test_request.rb; covers
    # the Response.decode shape guard at lib/kobako/transport/response.rb.
    bytes = Encoder.encode([0, nil, "extra"])
    assert_raises(InvalidType) { Envelope::Response.decode(bytes) }
  end

  def test_response_decode_error_requires_fault_payload
    # status=1 with a non-Fault value
    bytes = Encoder.encode([1, "stringy"])
    assert_raises(InvalidType) { Envelope::Response.decode(bytes) }
  end

  # ---------- Response golden vector ----------

  def test_response_ok_golden_for_int
    # Response: [0, 42]  =>  fixarray 2 (0x92) | 0x00 | 0x2a
    bytes = Envelope::Response.ok(42).encode
    assert_equal "92002a", hex(bytes)
  end

  # ============================================================
  # Cross-envelope nesting (Handle/Exception inside outer envelopes)
  # ============================================================

  def test_request_carrying_handle_and_response_carrying_handle
    # A transport call where the guest sends a Handle as both target and arg,
    # and the host responds with another Handle as the value.
    h_target = Handle.restore(10)
    h_arg    = Handle.restore(11)
    h_value  = Handle.restore(12)

    req = Envelope::Request.new(target: h_target, method_name: "save",
                                args: [h_arg], kwargs: {})
    decoded_req = Envelope::Request.decode(req.encode)
    assert_equal req, decoded_req

    resp = Envelope::Response.ok(h_value)
    decoded_resp = Envelope::Response.decode(resp.encode)
    assert_equal h_value, decoded_resp.payload
  end

  def test_response_error_with_fault_details
    exc = Exc.new(
      type: "argument",
      message: "bad",
      details: { "given" => [1, 2], "expected" => "string" }
    )
    resp = Envelope::Response.error(exc)
    decoded = Envelope::Response.decode(resp.encode)
    assert_equal exc, decoded.payload
  end
end
