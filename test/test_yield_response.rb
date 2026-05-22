# frozen_string_literal: true

# Unit tests for Kobako::Yield::Response and its codec helpers. Covers
# round-trip for the three live tags (0x01 ok / 0x02 break / 0x04 error)
# and the wire-violation paths (0x03 reserved, unknown tag, empty bytes).
# No native extension dependency — this exercises only the host-side
# value object plus the msgpack codec already covered elsewhere.

require "minitest/autorun"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/yield"

module Kobako
  class YieldResponseTest < Minitest::Test
    Y           = Kobako::Yield
    Response    = Kobako::Yield::Response
    Encoder     = Kobako::Codec::Encoder
    InvalidType = Kobako::Codec::InvalidType

    def hex(bytes)
      bytes.b.unpack1("H*")
    end

    # ------------------------------------------------------------
    # Construction validation
    # ------------------------------------------------------------

    def test_response_construction_rejects_reserved_and_unknown_tags
      [Y::TAG_RESERVED, 0x00, 0x05, 0xff].each do |bad|
        assert_raises(ArgumentError) { Response.new(tag: bad, value: nil) }
      end
    end

    # ------------------------------------------------------------
    # Round-trip per live tag
    # ------------------------------------------------------------

    def test_round_trip_ok_with_primitive
      resp = Response.new(tag: Y::TAG_OK, value: 42)
      decoded = Y.decode_response(Y.encode_response(resp))
      assert decoded.ok?
      assert_equal 42, decoded.value
    end

    def test_round_trip_break_with_symbol
      resp = Response.new(tag: Y::TAG_BREAK, value: :stop)
      decoded = Y.decode_response(Y.encode_response(resp))
      assert decoded.break?
      assert_equal :stop, decoded.value
    end

    def test_round_trip_error_with_class_message_backtrace
      payload = {
        "class" => "RuntimeError",
        "message" => "boom",
        "backtrace" => ["(eval):1:in `block'"]
      }
      resp = Response.new(tag: Y::TAG_ERROR, value: payload)
      decoded = Y.decode_response(Y.encode_response(resp))
      assert decoded.error?
      assert_equal payload, decoded.value
    end

    # ------------------------------------------------------------
    # Wire-violation paths
    # ------------------------------------------------------------

    def test_decode_rejects_reserved_tag_0x03
      # Forge bytes: tag 0x03 followed by msgpack nil.
      bytes = [Y::TAG_RESERVED].pack("C") + Encoder.encode(nil)
      err = assert_raises(InvalidType) { Y.decode_response(bytes) }
      assert_match(/reserved/i, err.message)
    end

    def test_decode_rejects_unknown_tag
      bytes = [0x7e].pack("C") + Encoder.encode(nil)
      assert_raises(InvalidType) { Y.decode_response(bytes) }
    end

    def test_decode_rejects_empty_bytes
      assert_raises(InvalidType) { Y.decode_response("".b) }
    end

    # ------------------------------------------------------------
    # Golden vectors — pin the byte layout against drift
    # ------------------------------------------------------------

    def test_encode_ok_with_int_42_golden
      bytes = Y.encode_response(Response.new(tag: Y::TAG_OK, value: 42))
      assert_equal "012a", hex(bytes) # tag 0x01 + msgpack int 42
    end

    def test_encode_break_with_nil_golden
      bytes = Y.encode_response(Response.new(tag: Y::TAG_BREAK, value: nil))
      assert_equal "02c0", hex(bytes) # tag 0x02 + msgpack nil
    end
  end
end
