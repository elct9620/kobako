# frozen_string_literal: true

require "test_helper"

# Wire-codec scalar round-trips (SPEC.md → Wire Codec → Type Mapping):
# nil / bool, every integer encoding tier and its overflow bound, float
# bit-fidelity, and the UTF-8 str vs binary bin distinction.
class TestCodecScalars < Minitest::Test
  include CodecHelpers

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
    assert_raises(UnsupportedType, "an integer past u64 max through Encoder.encode must raise UnsupportedType") do
      Encoder.encode(0x1_0000_0000_0000_0000)
    end
    assert_raises(UnsupportedType, "an integer below i64 min through Encoder.encode must raise UnsupportedType") do
      Encoder.encode(-0x8000_0000_0000_0001)
    end
  end

  # ---------- float ----------

  def test_float_special_values
    [0.0, -0.0, 1.0, -1.0, 0.1, 1e308, -1e308, Float::INFINITY, -Float::INFINITY].each do |f|
      _, decoded = roundtrip(f)
      assert_equal f, decoded, "float #{f} must round-trip with bit fidelity"
      # negative zero must preserve its sign bit
      assert_equal 1.0 / f, 1.0 / decoded, "signed zero must round-trip preserving its sign" if f.zero?
    end
  end

  def test_float_nan_preserves_nan_identity
    _, decoded = roundtrip(Float::NAN)
    assert_predicate decoded, :nan?, "NaN must round-trip as NaN"
  end

  # ---------- str / bin ----------

  def test_str_empty
    assert_roundtrip("")
  end

  def test_str_ascii
    s = "hello"
    _, decoded = roundtrip(s)
    assert_equal s, decoded, "an ASCII String must round-trip unchanged"
    assert_equal Encoding::UTF_8, decoded.encoding, "a str-family value must decode as UTF-8"
  end

  def test_str_multibyte_utf8
    s = "蒼時弦也こんにちは"
    _, decoded = roundtrip(s)
    assert_equal s, decoded, "a multibyte UTF-8 String must round-trip unchanged"
    assert_equal Encoding::UTF_8, decoded.encoding, "a multibyte str value must decode as UTF-8"
  end

  def test_str_long_crosses_str8_boundary
    # str 8 covers 32..255 bytes; verify both sides of the boundary.
    ["a" * 31, "a" * 32, "a" * 255, "a" * 256].each do |s|
      _, decoded = roundtrip(s)
      assert_equal s, decoded, "a #{s.bytesize}-byte String must round-trip across the str8 boundary"
    end
  end

  def test_str_long_crosses_str16_boundary
    ["a" * 0xffff, "a" * 0x1_0000].each do |s|
      _, decoded = roundtrip(s)
      assert_equal s, decoded, "a #{s.bytesize}-byte String must round-trip across the str16 boundary"
    end
  end

  def test_bin_non_utf8_bytes
    raw = [0xff, 0xfe, 0x00, 0x80].pack("C*") # invalid UTF-8
    _, decoded = roundtrip(raw)
    assert_equal raw, decoded, "non-UTF-8 bytes must round-trip unchanged through the bin family"
    assert_equal Encoding::ASCII_8BIT, decoded.encoding, "a bin-family value must decode as ASCII-8BIT"
  end

  def test_bin_explicit_binary_encoding
    s = "abc".b
    bytes = Encoder.encode(s)
    assert_equal 0xc4, bytes.getbyte(0), "an ASCII-8BIT String must encode with the bin8 marker (0xc4)"
    decoded = Decoder.decode(bytes)
    assert_equal Encoding::ASCII_8BIT, decoded.encoding, "a bin value must decode as ASCII-8BIT"
    assert_equal "abc".b, decoded, "the binary payload must round-trip unchanged"
  end
end
