# frozen_string_literal: true

# Unit + integration tests for the host-side envelope encoders/decoders
# (SPEC item #8). Builds on top of the byte-level wire codec covered by
# test/test_wire_codec.rb. No native extension dependency.

require "minitest/autorun"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/wire"

module Kobako
  module Wire
    class WireEnvelopeTest < Minitest::Test
      Envelope = Kobako::Wire::Envelope
      Handle   = Kobako::Wire::Handle
      Exc      = Kobako::Wire::Exception

      def hex(bytes)
        bytes.b.unpack1("H*")
      end

      # ============================================================
      # Request
      # ============================================================

      def test_request_construction_validates_field_types
        assert_raises(ArgumentError) { Envelope::Request.new(target: 123, method: "x") }
        assert_raises(ArgumentError) do
          Envelope::Request.new(target: "G::M", method: :sym)
        end
        assert_raises(ArgumentError) do
          Envelope::Request.new(target: "G::M", method: "x", args: "no")
        end
        assert_raises(ArgumentError) do
          Envelope::Request.new(target: "G::M", method: "x", kwargs: [])
        end
      end

      def test_request_round_trip_with_string_target
        req = Envelope::Request.new(
          target: "Store::Users",
          method: "find",
          args: [42, "alice"],
          kwargs: { "active" => true }
        )
        bytes   = Envelope.encode_request(req)
        decoded = Envelope.decode_request(bytes)
        assert_equal req, decoded
      end

      def test_request_round_trip_with_handle_target
        req = Envelope::Request.new(
          target: Handle.new(7),
          method: "save",
          args: [],
          kwargs: {}
        )
        bytes   = Envelope.encode_request(req)
        decoded = Envelope.decode_request(bytes)
        assert_equal req, decoded
        assert_instance_of Handle, decoded.target
      end

      def test_request_handles_in_args
        h1 = Handle.new(1)
        h2 = Handle.new(2)
        req = Envelope::Request.new(
          target: "G::M",
          method: "link",
          args: [h1, h2, "tag"],
          kwargs: { "k" => h1 }
        )
        decoded = Envelope.decode_request(Envelope.encode_request(req))
        assert_equal req, decoded
      end

      def test_request_kwargs_must_have_string_keys
        req = Envelope::Request.new(
          target: "G::M", method: "x", args: [], kwargs: { active: true }
        )
        assert_raises(InvalidType) { Envelope.encode_request(req) }
      end

      def test_request_three_field_signature
        # Convenience signature: encode_request(target, method, args, kwargs)
        bytes = Envelope.encode_request("G::M", "ping", [], {})
        decoded = Envelope.decode_request(bytes)
        assert_equal "G::M", decoded.target
        assert_equal "ping", decoded.method_name
        assert_empty decoded.args
        assert_empty decoded.kwargs
      end

      def test_request_decode_rejects_wrong_arity
        # 3-element array, not 4
        bytes = Encoder.encode(["G::M", "x", []])
        assert_raises(InvalidType) { Envelope.decode_request(bytes) }
      end

      # ---------- Request golden vector ----------

      def test_request_golden_empty_args_and_kwargs
        # Request: ["G::M", "ping", [], {}]
        # fixarray 4 (0x94) | fixstr 4 "G::M" (0xa4 47 3a 3a 4d) |
        # fixstr 4 "ping" (0xa4 70 69 6e 67) | fixarray 0 (0x90) | fixmap 0 (0x80)
        bytes = Envelope.encode_request("G::M", "ping", [], {})
        assert_equal "94a4473a3a4da470696e679080", hex(bytes)
      end

      # ============================================================
      # Response
      # ============================================================

      def test_response_ok_round_trip_with_primitive
        resp = Envelope::Response.ok(42)
        decoded = Envelope.decode_response(Envelope.encode_response(resp))
        assert decoded.ok?
        assert_equal 42, decoded.payload
      end

      def test_response_ok_round_trip_with_handle
        resp = Envelope::Response.ok(Handle.new(99))
        decoded = Envelope.decode_response(Envelope.encode_response(resp))
        assert decoded.ok?
        assert_instance_of Handle, decoded.payload
        assert_equal 99, decoded.payload.id
      end

      def test_response_err_round_trip
        exc = Exc.new(type: "runtime", message: "boom", details: nil)
        resp = Envelope::Response.err(exc)
        decoded = Envelope.decode_response(Envelope.encode_response(resp))
        assert decoded.err?
        assert_equal exc, decoded.payload
      end

      def test_response_err_requires_exception
        assert_raises(ArgumentError) { Envelope::Response.err("not an exc") }
      end

      def test_response_decode_rejects_unknown_status
        bytes = Encoder.encode([2, nil])
        assert_raises(InvalidType) { Envelope.decode_response(bytes) }
      end

      def test_response_decode_err_requires_exception_payload
        # status=1 with a non-Exception value
        bytes = Encoder.encode([1, "stringy"])
        assert_raises(InvalidType) { Envelope.decode_response(bytes) }
      end

      # ---------- Response golden vector ----------

      def test_response_ok_golden_for_42
        # Response: [0, 42]  =>  fixarray 2 (0x92) | 0x00 | 0x2a
        bytes = Envelope.encode_response(Envelope::Response.ok(42))
        assert_equal "92002a", hex(bytes)
      end

      # ============================================================
      # Result envelope
      # ============================================================

      def test_result_round_trip_primitive
        bytes = Envelope.encode_result(42)
        result = Envelope.decode_result(bytes)
        assert_equal 42, result.value
      end

      def test_result_round_trip_nil
        bytes = Envelope.encode_result(nil)
        result = Envelope.decode_result(bytes)
        assert_nil result.value
      end

      def test_result_round_trip_handle
        h = Handle.new(5)
        bytes = Envelope.encode_result(h)
        result = Envelope.decode_result(bytes)
        assert_equal h, result.value
      end

      def test_result_round_trip_complex_value
        v = { "list" => [1, 2.5, "three"], "nested" => { "ok" => true } }
        result = Envelope.decode_result(Envelope.encode_result(v))
        assert_equal v, result.value
      end

      # ---------- Result golden vector (matches Rust codec test) ----------

      def test_result_golden_value_42
        # SPEC.md "Outcome Envelope" example: fixarray len=1 + 42.
        bytes = Envelope.encode_result(42)
        assert_equal "912a", hex(bytes)
      end

      # ============================================================
      # Panic envelope
      # ============================================================

      def test_panic_round_trip_minimum_fields
        panic = Envelope::Panic.new(
          origin: "sandbox",
          klass: "RuntimeError",
          message: "boom"
        )
        decoded = Envelope.decode_panic(Envelope.encode_panic(panic))
        assert_equal panic, decoded
      end

      def test_panic_round_trip_with_backtrace_and_details
        panic = Envelope::Panic.new(
          origin: "service",
          klass: "Kobako::ServiceError",
          message: "service failed",
          backtrace: ["a.rb:1", "b.rb:2"],
          details: { "type" => "runtime" }
        )
        decoded = Envelope.decode_panic(Envelope.encode_panic(panic))
        assert_equal panic, decoded
      end

      def test_panic_decode_silently_ignores_unknown_keys
        bytes = Encoder.encode({
                                 "origin" => "sandbox",
                                 "class" => "RuntimeError",
                                 "message" => "boom",
                                 "future_key" => "ignored"
                               })
        decoded = Envelope.decode_panic(bytes)
        assert_equal "sandbox", decoded.origin
        assert_equal "RuntimeError", decoded.klass
      end

      def test_panic_decode_rejects_missing_required_key
        bytes = Encoder.encode({ "origin" => "sandbox", "message" => "boom" })
        assert_raises(InvalidType) { Envelope.decode_panic(bytes) }
      end

      def test_panic_decode_rejects_non_map_payload
        bytes = Encoder.encode([1, 2, 3])
        assert_raises(InvalidType) { Envelope.decode_panic(bytes) }
      end

      def test_panic_decode_rejects_non_string_backtrace_lines
        bytes = Encoder.encode({
                                 "origin" => "sandbox",
                                 "class" => "RuntimeError",
                                 "message" => "boom",
                                 "backtrace" => ["ok", 42]
                               })
        assert_raises(InvalidType) { Envelope.decode_panic(bytes) }
      end

      def test_panic_construction_validates_field_types
        assert_raises(ArgumentError) { Envelope::Panic.new(origin: 123,       klass: "E", message: "m") }
        assert_raises(ArgumentError) { Envelope::Panic.new(origin: "sandbox", klass: :sym, message: "m") }
        assert_raises(ArgumentError) { Envelope::Panic.new(origin: "sandbox", klass: "E", message: nil) }
        assert_raises(ArgumentError) do
          Envelope::Panic.new(origin: "sandbox", klass: "E", message: "m", backtrace: "str")
        end
      end

      # ============================================================
      # Outcome envelope (tagged wrapper)
      # ============================================================

      def test_outcome_result_round_trip
        outcome = Envelope::Outcome.result(123)
        bytes   = Envelope.encode_outcome(outcome)
        assert_equal Envelope::OUTCOME_TAG_RESULT, bytes.getbyte(0)
        decoded = Envelope.decode_outcome(bytes)
        assert decoded.result?
        assert_equal 123, decoded.payload.value
      end

      def test_outcome_panic_round_trip
        panic = Envelope::Panic.new(
          origin: "sandbox", klass: "RuntimeError", message: "boom"
        )
        outcome = Envelope::Outcome.panic(panic)
        bytes   = Envelope.encode_outcome(outcome)
        assert_equal Envelope::OUTCOME_TAG_PANIC, bytes.getbyte(0)
        decoded = Envelope.decode_outcome(bytes)
        assert decoded.panic?
        assert_equal panic, decoded.payload
      end

      def test_outcome_decode_rejects_unknown_tag
        assert_raises(InvalidType) { Envelope.decode_outcome("\x03\x90".b) }
      end

      def test_outcome_decode_rejects_empty_bytes
        assert_raises(InvalidType) { Envelope.decode_outcome("".b) }
      end

      # ---------- Outcome golden vector (matches Rust codec test) ----------

      def test_outcome_result_golden_for_42
        # Tag 0x01 + Result envelope (fixarray 1, 0x2a)
        bytes = Envelope.encode_outcome(Envelope::Outcome.result(42))
        assert_equal "01912a", hex(bytes)
      end

      def test_outcome_panic_golden_minimum
        # Tag 0x02 + fixmap 3 with origin=sandbox, class=RuntimeError, message=boom
        panic = Envelope::Panic.new(origin: "sandbox", klass: "RuntimeError", message: "boom")
        bytes = Envelope.encode_outcome(Envelope::Outcome.panic(panic))
        # 02 | 83 | a6 origin     a7 sandbox          | a5 class    ac RuntimeError                       | a7 message a4 boom
        expected = "02" \
                   "83" \
                   "a66f726967696ea773616e64626f78" \
                   "a5636c617373ac52756e74696d654572726f72" \
                   "a76d657373616765a4626f6f6d"
        assert_equal expected, hex(bytes)
      end

      # ============================================================
      # Cross-envelope nesting (Handle/Exception inside outer envelopes)
      # ============================================================

      def test_request_carrying_handle_and_response_carrying_handle
        # An RPC where the guest sends a Handle as both target and arg,
        # and the host responds with another Handle as the value.
        h_target = Handle.new(10)
        h_arg    = Handle.new(11)
        h_value  = Handle.new(12)

        req = Envelope::Request.new(target: h_target, method: "save",
                                    args: [h_arg], kwargs: {})
        decoded_req = Envelope.decode_request(Envelope.encode_request(req))
        assert_equal req, decoded_req

        resp = Envelope::Response.ok(h_value)
        decoded_resp = Envelope.decode_response(Envelope.encode_response(resp))
        assert_equal h_value, decoded_resp.payload
      end

      def test_response_err_with_exception_details
        exc = Exc.new(
          type: "argument",
          message: "bad",
          details: { "given" => [1, 2], "expected" => "string" }
        )
        resp = Envelope::Response.err(exc)
        decoded = Envelope.decode_response(Envelope.encode_response(resp))
        assert_equal exc, decoded.payload
      end
    end
  end
end
