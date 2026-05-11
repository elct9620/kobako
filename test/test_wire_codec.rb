# frozen_string_literal: true

# E2E + integration test for the pure-Ruby host wire codec (SPEC item #5).
#
# Intentionally does NOT require "test_helper" — like the other clean-checkout
# tests in this suite, the codec must be exercisable without the native
# extension being compiled. We require lib/kobako/wire.rb directly.

require "minitest/autorun"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/wire"

module Kobako
  module Wire
    class WireCodecTest < Minitest::Test
      Encoder  = Kobako::Wire::Encoder
      Decoder  = Kobako::Wire::Decoder
      Handle   = Kobako::Wire::Handle
      Exc      = Kobako::Wire::Exception

      # ---------- helpers ----------

      def roundtrip(value)
        bytes = Encoder.encode(value)
        decoded = Decoder.new(bytes).read
        [bytes, decoded]
      end

      def assert_roundtrip(value)
        _, decoded = roundtrip(value)
        if value.nil?
          assert_nil decoded, "round-trip mismatch for nil"
        else
          assert_equal value, decoded, "round-trip mismatch for #{value.inspect}"
        end
      end

      def hex(bytes)
        bytes.b.unpack1("H*")
      end

      def assert_bytes(expected_hex, value)
        bytes = Encoder.encode(value)
        assert_equal expected_hex, hex(bytes),
                     "encoding mismatch for #{value.inspect}"
      end

      # ---------- nil / bool ----------

      def test_nil_roundtrip
        assert_roundtrip(nil)
      end

      def test_true_roundtrip
        assert_roundtrip(true)
      end

      def test_false_roundtrip
        assert_roundtrip(false)
      end

      # ---------- integer boundaries ----------

      def test_integer_fixint_boundaries
        # positive fixint: 0..127
        [0, 1, 0x7f].each { |n| assert_roundtrip(n) }
        # negative fixint: -32..-1
        [-1, -32].each { |n| assert_roundtrip(n) }
      end

      def test_integer_uint_boundaries
        [0x80, 0xff,                              # uint8
         0x100, 0xffff,                           # uint16
         0x1_0000, 0xffff_ffff,                   # uint32
         0x1_0000_0000, 0xffff_ffff_ffff_ffff].each do |n| # uint64
          assert_roundtrip(n)
        end
      end

      def test_integer_int_boundaries
        [-33, -0x80,                              # int8
         -0x81, -0x8000,                          # int16
         -0x8001, -0x8000_0000,                   # int32
         -0x8000_0001, -0x8000_0000_0000_0000].each do |n| # int64
          assert_roundtrip(n)
        end
      end

      def test_integer_overflow_raises
        assert_raises(UnsupportedType) { Encoder.encode(0x1_0000_0000_0000_0000) }
        assert_raises(UnsupportedType) { Encoder.encode(-0x8000_0000_0000_0001) }
      end

      # ---------- float ----------

      def test_float_special_values
        [0.0, -0.0, 1.0, -1.0, 0.1, 1e308, -1e308, Float::INFINITY, -Float::INFINITY].each do |f|
          _, decoded = roundtrip(f)
          assert_equal f, decoded
          # negative zero must preserve sign bit
          assert_equal 1.0 / f, 1.0 / decoded if f.zero?
        end
      end

      def test_float_nan_preserves_nan_identity
        _, decoded = roundtrip(Float::NAN)
        assert_predicate decoded, :nan?
      end

      # ---------- str / bin ----------

      def test_str_empty
        assert_roundtrip("")
      end

      def test_str_ascii
        s = "hello"
        _, decoded = roundtrip(s)
        assert_equal s, decoded
        assert_equal Encoding::UTF_8, decoded.encoding
      end

      def test_str_multibyte_utf8
        s = "蒼時弦也こんにちは"
        _, decoded = roundtrip(s)
        assert_equal s, decoded
        assert_equal Encoding::UTF_8, decoded.encoding
      end

      def test_str_long_crosses_str8_boundary
        # str 8 covers 32..255 bytes; verify both sides of the boundary.
        ["a" * 31, "a" * 32, "a" * 255, "a" * 256].each do |s|
          _, decoded = roundtrip(s)
          assert_equal s, decoded
        end
      end

      def test_str_long_crosses_str16_boundary
        ["a" * 0xffff, "a" * 0x1_0000].each do |s|
          _, decoded = roundtrip(s)
          assert_equal s, decoded
        end
      end

      def test_bin_non_utf8_bytes
        raw = [0xff, 0xfe, 0x00, 0x80].pack("C*") # invalid UTF-8
        _, decoded = roundtrip(raw)
        assert_equal raw, decoded
        assert_equal Encoding::ASCII_8BIT, decoded.encoding
      end

      def test_bin_explicit_binary_encoding
        s = "abc".b
        bytes = Encoder.encode(s)
        # 0xc4 = bin8
        assert_equal 0xc4, bytes.getbyte(0)
        decoded = Decoder.new(bytes).read
        assert_equal Encoding::ASCII_8BIT, decoded.encoding
        assert_equal "abc".b, decoded
      end

      # ---------- array ----------

      def test_array_empty
        assert_roundtrip([])
      end

      def test_array_mixed_types
        a = [nil, true, false, 1, -1, 1.5, "x", "y".b, [1, 2], { "k" => "v" }]
        assert_roundtrip(a)
      end

      def test_array_nested
        assert_roundtrip([[[[[42]]]]])
      end

      def test_array_crosses_array16_boundary
        [Array.new(15, 0), Array.new(16, 0), Array.new(0xffff, 0), Array.new(0x1_0000, 0)].each do |a|
          _, decoded = roundtrip(a)
          assert_equal a, decoded
        end
      end

      # ---------- map ----------

      def test_map_empty
        assert_roundtrip({})
      end

      def test_map_string_keys
        assert_roundtrip({ "a" => 1, "b" => 2, "c" => nil })
      end

      def test_map_non_string_keys
        # SPEC envelope rules forbid this in specific positions, but the
        # codec itself must handle arbitrary wire-legal keys.
        assert_roundtrip({ 1 => "one", 2 => "two", true => "t" })
      end

      def test_map_nested
        assert_roundtrip({ "outer" => { "inner" => { "leaf" => [1, 2, 3] } } })
      end

      # ---------- ext 0x01 Handle ----------

      def test_handle_roundtrip_min
        h = Handle.new(1)
        _, decoded = roundtrip(h)
        assert_equal h, decoded
      end

      def test_handle_roundtrip_max
        h = Handle.new(Handle::MAX_ID)
        _, decoded = roundtrip(h)
        assert_equal h, decoded
      end

      def test_handle_zero_id_rejected_at_construction
        assert_raises(ArgumentError) { Handle.new(0) }
      end

      def test_handle_over_cap_rejected_at_construction
        assert_raises(ArgumentError) { Handle.new(Handle::MAX_ID + 1) }
      end

      def test_handle_zero_id_on_wire_rejected
        # Manually construct fixext4 + 0x01 + zero ID
        bytes = "\xd6\x01\x00\x00\x00\x00".b
        assert_raises(InvalidType) { Decoder.new(bytes).read }
      end

      def test_handle_over_cap_on_wire_rejected
        bytes = "\xd6\x01\x80\x00\x00\x00".b
        assert_raises(InvalidType) { Decoder.new(bytes).read }
      end

      # ---------- ext 0x02 Exception ----------

      def test_exception_roundtrip_minimal
        e = Exc.new(type: "runtime", message: "boom")
        _, decoded = roundtrip(e)
        assert_equal e, decoded
      end

      def test_exception_roundtrip_with_details
        e = Exc.new(type: "argument", message: "bad arg",
                    details: { "field" => "x", "expected" => "Integer" })
        _, decoded = roundtrip(e)
        assert_equal e, decoded
      end

      def test_exception_all_valid_types
        Exc::VALID_TYPES.each do |t|
          e = Exc.new(type: t, message: "m")
          _, decoded = roundtrip(e)
          assert_equal e, decoded
        end
      end

      def test_exception_invalid_type_rejected_at_construction
        assert_raises(ArgumentError) { Exc.new(type: "fatal", message: "m") }
      end

      # ---------- deep nesting ----------

      def test_deeply_nested_mixed
        h = Handle.new(7)
        e = Exc.new(type: "undefined", message: "missing")
        value = [
          { "handles" => [h, h], "errors" => [e] },
          [{ "deep" => [{ "deeper" => [h] }] }]
        ]
        _, decoded = roundtrip(value)
        assert_equal value, decoded
      end

      # ---------- decoder error cases ----------

      def test_truncated_empty_input
        assert_raises(Truncated) { Decoder.new("".b).read }
      end

      def test_truncated_in_str_payload
        # fixstr len=5 but only 2 bytes follow
        bytes = "\xa5ab".b
        assert_raises(Truncated) { Decoder.new(bytes).read }
      end

      def test_truncated_in_int64
        bytes = "\xcf\x00\x00\x00".b
        assert_raises(Truncated) { Decoder.new(bytes).read }
      end

      def test_invalid_type_tag
        # 0xc1 is reserved as "never used" in msgpack -> wire violation
        bytes = "\xc1".b
        assert_raises(InvalidType) { Decoder.new(bytes).read }
      end

      def test_unknown_ext_code_rejected
        # fixext1 with type 0x99 (not 0x01 or 0x02)
        bytes = "\xd4\x99\x00".b
        assert_raises(InvalidType) { Decoder.new(bytes).read }
      end

      def test_invalid_utf8_in_str_rejected
        # fixstr len=2 with invalid UTF-8 bytes (lone continuation byte)
        bytes = "\xa2\xff\xfe".b
        assert_raises(InvalidEncoding) { Decoder.new(bytes).read }
      end

      def test_unsupported_ruby_type_at_encode
        assert_raises(UnsupportedType) { Encoder.encode(:a_symbol) }
        assert_raises(UnsupportedType) { Encoder.encode(Object.new) }
      end

      # ---------- bytes-level golden vectors (SPEC compliance) ----------
      #
      # These vectors fix the encoder output to specific byte sequences so
      # the test acts as a real spec-compliance check, not a self-consistency
      # check. Hex is shown without separators for compact equality.

      def test_golden_vector_nil
        # 0xc0 = msgpack nil
        assert_bytes "c0", nil
      end

      def test_golden_vector_positive_fixint
        # 42 is a positive fixint -> single byte 0x2a
        assert_bytes "2a", 42
      end

      def test_golden_vector_negative_fixint
        # -1 -> negative fixint 0xff
        assert_bytes "ff", -1
      end

      def test_golden_vector_fixstr_hello
        # "hello" -> 0xa5 'h' 'e' 'l' 'l' 'o'
        assert_bytes "a568656c6c6f", "hello"
      end

      def test_golden_vector_fixarray_with_positive_fixint
        # [42] -> 0x91 0x2a (fixarray len=1, positive fixint 42)
        # Matches the SPEC's tag-0x01 example bytes "91 2a".
        assert_bytes "912a", [42]
      end

      def test_golden_vector_handle
        # Handle(1) -> fixext4 ext 0x01 + big-endian u32 1
        # 0xd6 0x01 0x00 0x00 0x00 0x01
        assert_bytes "d60100000001", Handle.new(1)
      end

      def test_golden_vector_handle_max
        # Handle(0x7fff_ffff) -> 0xd6 0x01 0x7f 0xff 0xff 0xff
        assert_bytes "d6017fffffff", Handle.new(Handle::MAX_ID)
      end

      def test_golden_vector_exception_minimal
        # Exception(type: "runtime", message: "boom", details: nil)
        # Inner map (3 entries) bytes:
        #   83                          fixmap len=3
        #   a4 74 79 70 65              fixstr "type"
        #   a7 72 75 6e 74 69 6d 65     fixstr "runtime"
        #   a7 6d 65 73 73 61 67 65     fixstr "message"
        #   a4 62 6f 6f 6d              fixstr "boom"
        #   a7 64 65 74 61 69 6c 73     fixstr "details"
        #   c0                          nil
        # Total inner length = 1 + 5 + 8 + 8 + 5 + 8 + 1 = 36 bytes
        # Outer framing: 0xc7 0x24 0x02 + inner
        inner_hex = "83" \
                    "a474797065a772756e74696d65" \
                    "a76d657373616765a4626f6f6d" \
                    "a764657461696c73c0"
        expected = "c72402#{inner_hex}"
        assert_bytes expected, Exc.new(type: "runtime", message: "boom")
      end

      # ---------- golden vectors: narrow zero-length tags (SPEC Wire Codec §Type Mapping) ----------
      #
      # Each empty container must encode to its narrowest possible tag — the
      # single-byte "fix" form.  These vectors prevent silent drift toward a
      # wider format (e.g. str8 for empty string) that would break the Rust
      # guest codec's decoder expectations.

      def test_golden_vector_empty_str
        # "" -> fixstr len=0 -> 0xa0 (no payload bytes)
        assert_bytes "a0", ""
      end

      def test_golden_vector_empty_bin
        # "".b -> bin8 len=0 -> 0xc4 0x00
        assert_bytes "c400", "".b
      end

      def test_golden_vector_empty_array
        # [] -> fixarray len=0 -> 0x90
        assert_bytes "90", []
      end

      def test_golden_vector_empty_map
        # {} -> fixmap len=0 -> 0x80
        assert_bytes "80", {}
      end

      # ---------- golden vectors: integer boundary tags (SPEC Wire Codec §Type Mapping) ----------
      #
      # These pin the exact tag byte at each encoding tier boundary so a
      # future encoder change that silently promotes to a wider format is
      # caught as a golden-vector mismatch.

      def test_golden_vector_zero_positive_fixint
        # 0 -> positive fixint -> 0x00
        assert_bytes "00", 0
      end

      def test_golden_vector_max_positive_fixint
        # 127 -> positive fixint -> 0x7f (last positive-fixint value)
        assert_bytes "7f", 127
      end

      def test_golden_vector_min_negative_fixint
        # -32 -> negative fixint -> 0xe0 (first negative-fixint value = 0b111_00000)
        assert_bytes "e0", -32
      end

      # ---------- Handle ext wrong payload length (SPEC Wire Codec §Ext Types) ----------
      #
      # The factory's decode_handle validates that the ext 0x01 payload is
      # exactly 4 bytes.  A fixext1 (0xd4 type=0x01, 1-byte payload) is a
      # deliberate wire violation that must raise InvalidType, not silently
      # decode as a Handle with a truncated id.

      def test_handle_wrong_payload_length_on_wire_rejected
        # fixext1: 0xd4  type=0x01  payload=0x01 (1 byte instead of 4)
        bytes = "\xd4\x01\x01".b
        err = assert_raises(InvalidType) { Decoder.new(bytes).read }
        assert_match(/4 bytes/, err.message)
      end

      # ---------- self-consistency: every type goes through one big fuzz-ish list ----------

      def test_combined_payload_roundtrip
        value = combined_payload_fixture
        _, decoded = roundtrip(value)
        # Float::INFINITY and -0.0 compare equal under == so plain assert_equal works
        assert_equal value, decoded
      end

      def combined_payload_fixture
        h = Handle.new(123_456)
        e = Exc.new(type: "runtime", message: "x", details: [1, 2, 3])
        combined_payload_without_ext_types.merge(
          "map" => { "nested" => { "again" => [h] } },
          "handle" => h,
          "exc" => e
        )
      end

      def combined_payload_without_ext_types
        {
          "nil" => nil,
          "bools" => [true, false],
          "ints" => [-1, 0, 1, 0x7f, 0x80, 0xffff_ffff_ffff_ffff, -0x8000_0000_0000_0000],
          "floats" => [0.0, -0.0, 1.5, Float::INFINITY],
          "strs" => ["", "a", "蒼"],
          "bins" => [[0xff, 0x00].pack("C*")],
          "arr" => [[], [[]], [[[]]]]
        }
      end

      # Migration check: codec backbone is the official `msgpack` gem.
      # If someone quietly reverts to a hand-rolled implementation, this
      # test catches the drift via the gem's class hierarchy.
      def test_msgpack_factory_is_the_codec_backbone
        require "msgpack"
        assert defined?(::MessagePack::Factory),
               "msgpack gem must be loaded — wire codec is built on MessagePack::Factory"
        factory = Kobako::Wire::Factory.instance
        assert_kind_of ::MessagePack::Factory, factory,
                       "Kobako::Wire::Factory.instance must be a MessagePack::Factory"
      end
    end
  end
end
