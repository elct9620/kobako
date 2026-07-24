# frozen_string_literal: true

require "test_helper"

# Wire-codec ext-type round-trips and validation (SPEC.md → Wire Codec →
# Ext Types): Symbol (ext 0x00), Kobako::Handle (ext 0x01) with its id
# bounds and payload-length checks, and Kobako::Fault (ext 0x02) with its
# closed type taxonomy.
class TestCodecExtTypes < Minitest::Test
  include CodecHelpers

  # ---------- ext 0x00 Symbol ----------

  def test_symbol_roundtrip_payload_sizes
    # Empty Symbol is wire-legal; multibyte UTF-8 must survive.
    [:hello, :"", :蒼時].each { |s| assert_roundtrip(s) }
  end

  def test_symbol_preserved_across_string_distinction
    # SPEC: a str/bin value carrying the bytes of a symbol's name is
    # NOT wire-equivalent to that Symbol; both sides must remain
    # distinguishable end-to-end.
    _, decoded_sym = roundtrip(:foo)
    _, decoded_str = roundtrip("foo")
    assert_kind_of Symbol, decoded_sym, "a Symbol must decode back as a Symbol, not its String name"
    assert_kind_of String, decoded_str, "a String must decode back as a String"
    refute_equal decoded_sym, decoded_str, "a Symbol and the String of its name must stay distinguishable on the wire"
  end

  def test_invalid_utf8_in_symbol_rejected
    # ext 0x00 payload must decode as UTF-8 — SPEC forbids the
    # binary-encoded Symbol fallback.
    bytes = "\xc7\x02\x00\xff\xfe".b
    assert_raises(InvalidEncoding, "a Symbol (ext 0x00) with non-UTF-8 payload bytes must raise InvalidEncoding") do
      Decoder.decode(bytes)
    end
  end

  # ---------- ext 0x01 Handle ----------

  def test_handle_roundtrip_min
    h = Handle.restore(1)
    _, decoded = roundtrip(h)
    assert_equal h, decoded, "the minimum Handle id (1) must round-trip unchanged"
  end

  def test_handle_roundtrip_max
    h = Handle.restore(Handle::MAX_ID)
    _, decoded = roundtrip(h)
    assert_equal h, decoded, "the maximum Handle id must round-trip unchanged"
  end

  def test_handle_zero_id_rejected_at_construction
    assert_raises(ArgumentError, "Handle.restore(0) must raise ArgumentError — id 0 is the reserved sentinel") do
      Handle.restore(0)
    end
  end

  def test_handle_over_cap_rejected_at_construction
    assert_raises(ArgumentError, "Handle.restore past MAX_ID must raise ArgumentError") do
      Handle.restore(Handle::MAX_ID + 1)
    end
  end

  def test_handle_zero_id_on_wire_rejected
    # Manually construct fixext4 + 0x01 + zero ID
    bytes = "\xd6\x01\x00\x00\x00\x00".b
    assert_raises(InvalidType, "a wire Handle with id 0 must be rejected as InvalidType") do
      Decoder.decode(bytes)
    end
  end

  def test_handle_over_cap_on_wire_rejected
    bytes = "\xd6\x01\x80\x00\x00\x00".b
    assert_raises(InvalidType, "a wire Handle id past the cap must be rejected as InvalidType") do
      Decoder.decode(bytes)
    end
  end

  # The factory's unpack_handle validates that the ext 0x01 payload is
  # exactly 4 bytes.  A fixext1 (0xd4 type=0x01, 1-byte payload) is a
  # deliberate wire violation that must raise InvalidType, not silently
  # decode as a Handle with a truncated id.
  def test_handle_wrong_payload_length_on_wire_rejected
    # fixext1: 0xd4  type=0x01  payload=0x01 (1 byte instead of 4)
    bytes = "\xd4\x01\x01".b
    err = assert_raises(InvalidType, "a Handle ext with a non-4-byte payload must be rejected as InvalidType") do
      Decoder.decode(bytes)
    end
    assert_match(/4 bytes/, err.message, "the rejection must name the required 4-byte Handle payload length")
  end

  # ---------- ext 0x02 Exception ----------

  def test_exception_roundtrip_minimal
    e = Exc.new(type: "runtime", message: "boom")
    _, decoded = roundtrip(e)
    assert_equal e, decoded, "a minimal Fault (ext 0x02) must round-trip unchanged"
  end

  def test_exception_roundtrip_with_details
    e = Exc.new(type: "argument", message: "bad arg",
                details: { "field" => "x", "expected" => "Integer" })
    _, decoded = roundtrip(e)
    assert_equal e, decoded, "a Fault with a nested details Hash must round-trip unchanged"
  end

  def test_exception_all_valid_types
    Exc::VALID_TYPES.each do |t|
      e = Exc.new(type: t, message: "m")
      _, decoded = roundtrip(e)
      assert_equal e, decoded, "a Fault of type #{t.inspect} must round-trip unchanged"
    end
  end

  def test_exception_invalid_type_rejected_at_construction
    assert_raises(ArgumentError, "a Fault type outside the closed taxonomy must raise ArgumentError") do
      Exc.new(type: "fatal", message: "m")
    end
  end
end
