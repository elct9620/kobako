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

  # The validation walk must cover both halves of every map entry — a
  # regression skipping keys or values stays green on the top-level
  # fixstr case above.
  def test_invalid_utf8_in_map_key_rejected
    # fixmap1 { fixstr2 <invalid> => fixint 1 }
    bytes = "\x81\xa2\xff\xfe\x01".b
    assert_raises(InvalidEncoding) { Decoder.decode(bytes) }
  end

  def test_invalid_utf8_in_map_value_rejected
    # fixmap1 { fixstr1 "a" => fixstr2 <invalid> }
    bytes = "\x81\xa1a\xa2\xff\xfe".b
    assert_raises(InvalidEncoding) { Decoder.decode(bytes) }
  end

  def test_unsupported_ruby_type_at_encode
    # SPEC's 12-entry mapping is closed; types outside it (Object,
    # Range, Time, ...) raise UnsupportedType.
    assert_raises(UnsupportedType) { Encoder.encode(Object.new) }
  end

  # Decoder-wide half of the single-msgpack-value rule (SPEC.md § Wire
  # Codec): envelope-level rejection is pinned per envelope (e.g.
  # test/unit/transport/test_request.rb); this case pins that the property
  # comes from the Decoder itself, for every payload shape.
  def test_trailing_bytes_after_a_complete_value_rejected
    bytes = Encoder.encode(42) + Encoder.encode(nil)
    assert_raises(InvalidType,
                  "bytes past one complete msgpack value through Decoder.decode must be a wire violation") do
      Decoder.decode(bytes)
    end
  end

  # A Fault occupies the whole of a Reply's fault body, so everything
  # inside one is a payload position where a Fault is illegal (E-50). A
  # hostile guest that chains ext 0x02 through the inner map's values gets
  # a decoder re-entry per level, each with a fresh msgpack unpacker whose
  # stack guard resets — deep enough input would exhaust the Ruby stack and
  # escape the codec's rescue, since SystemStackError is not a Codec::Error.
  # Refusing the second level is what keeps the chain from ever forming.
  def test_nested_fault_rejected
    assert_raises(InvalidType,
                  "ext 0x02 nested inside a Fault through #decode must raise InvalidType, not trap the stack") do
      Decoder.decode(nested_fault_bytes(200))
    end
  end

  # The payload-position flag is unwound by an ensure, so a rejected chain
  # must leave no residue that trips a later decode on the same thread —
  # otherwise one bad payload would poison every subsequent invocation
  # sharing that thread.
  def test_nested_fault_rejection_leaves_no_residue
    assert_raises(InvalidType) { Decoder.decode(nested_fault_bytes(200)) }
    decoded = Decoder.decode(Encoder.encode(Kobako::Fault.new(type: "runtime", message: "x")))
    assert_instance_of Kobako::Fault, decoded,
                       "a lone Fault decoded after a rejected nested chain must still succeed"
  end

  private

  # Frame +payload_bytes+ (a msgpack map) as an ext 0x02 Fault.
  # ext 32 (0xc9) keeps the length field wide enough for the growing
  # nested chain.
  def ext_fault(payload_bytes)
    [0xc9, payload_bytes.bytesize, 0x02].pack("CNC").b + payload_bytes
  end

  # A fixmap-2 { "type" => <nested>, "message" => "x" } smuggling the
  # already-encoded inner bytes into the position a type string belongs in.
  def fault_map(nested_bytes)
    "\x82\xa4type".b + nested_bytes + "\xa7message\xa1x".b
  end

  # Wire bytes for +depth+ ext 0x02 Faults chained through each other,
  # the innermost carrying nil where a nested Fault would sit.
  def nested_fault_bytes(depth)
    depth.times.reduce("\xc0".b) { |inner, _| ext_fault(fault_map(inner)) }
  end
end
