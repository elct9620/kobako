# frozen_string_literal: true

require "test_helper"

# Wire-codec rejection paths (SPEC.md → Wire Codec): truncated input,
# reserved / unknown tags, invalid UTF-8 in str, and the closed 12-entry
# type mapping at encode time. Every violation surfaces through the
# Kobako::Codec error taxonomy, never a raw Ruby failure.
class TestCodecMalformed < Minitest::Test
  include CodecHelpers

  def test_truncated_empty_input
    assert_raises(Truncated) { Decoder.decode("".b) }
  end

  def test_truncated_in_str_payload
    # fixstr len=5 but only 2 bytes follow
    bytes = "\xa5ab".b
    assert_raises(Truncated) { Decoder.decode(bytes) }
  end

  def test_truncated_in_int64
    bytes = "\xcf\x00\x00\x00".b
    assert_raises(Truncated) { Decoder.decode(bytes) }
  end

  def test_invalid_type_tag
    # 0xc1 is reserved as "never used" in msgpack -> wire violation
    bytes = "\xc1".b
    assert_raises(InvalidType) { Decoder.decode(bytes) }
  end

  def test_unknown_ext_code_rejected
    # fixext1 with type 0x99 (not 0x01 or 0x02)
    bytes = "\xd4\x99\x00".b
    assert_raises(InvalidType) { Decoder.decode(bytes) }
  end

  def test_invalid_utf8_in_str_rejected
    # fixstr len=2 with invalid UTF-8 bytes (lone continuation byte)
    bytes = "\xa2\xff\xfe".b
    assert_raises(InvalidEncoding) { Decoder.decode(bytes) }
  end

  def test_unsupported_ruby_type_at_encode
    # SPEC's 12-entry mapping is closed; types outside it (Object,
    # Range, Time, ...) raise UnsupportedType.
    assert_raises(UnsupportedType) { Encoder.encode(Object.new) }
  end
end
