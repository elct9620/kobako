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
      Envelope    = Kobako::Wire::Envelope
      Handle      = Kobako::RPC::Handle
      Exc         = Kobako::Wire::Exception
      Encoder     = Kobako::Codec::Encoder
      InvalidType = Kobako::Codec::InvalidType

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
          kwargs: { active: true }
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
          kwargs: { k: h1 }
        )
        decoded = Envelope.decode_request(Envelope.encode_request(req))
        assert_equal req, decoded
      end

      def test_request_kwargs_must_have_symbol_keys
        # SPEC.md → Wire Codec → Ext Types → ext 0x00 pins kwargs keys
        # to Symbols. The Value Object refuses non-Symbol kwargs keys at
        # construction; the wire-level InvalidType guarantee is preserved
        # via the decode_request boundary translator.
        assert_raises(ArgumentError) do
          Envelope::Request.new(target: "G::M", method: "x", args: [], kwargs: { "active" => true })
        end
      end

      def test_request_decode_translates_non_symbol_kwargs_key_to_invalid_type
        # Forge wire bytes with an int kwargs key — msgpack-legal but
        # envelope-illegal. The decoder must translate the value-object
        # ArgumentError into a wire-layer InvalidType.
        bytes = Encoder.encode(["G::M", "x", [], { 42 => "v" }])
        assert_raises(InvalidType) { Envelope.decode_request(bytes) }
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
        bytes = Envelope.encode_request(Envelope::Request.new(target: "G::M", method: "ping"))
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

      def test_response_construction_validates_field_types
        assert_raises(ArgumentError) { Envelope::Response.new(status: 99,                        payload: nil) }
        assert_raises(ArgumentError) { Envelope::Response.new(status: -1,                        payload: nil) }
        assert_raises(ArgumentError) { Envelope::Response.new(status: Envelope::STATUS_ERROR, payload: "str") }
        assert_raises(ArgumentError) { Envelope::Response.new(status: Envelope::STATUS_ERROR, payload: 42) }
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

      def test_response_ok_golden_for_int
        # Response: [0, 42]  =>  fixarray 2 (0x92) | 0x00 | 0x2a
        bytes = Envelope.encode_response(Envelope::Response.ok(42))
        assert_equal "92002a", hex(bytes)
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
