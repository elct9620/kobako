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

  # A hostile guest can chain ext 0x02 (Fault) envelopes through each
  # other's +details+ field. Every nested Fault re-enters the decoder with
  # a fresh msgpack unpacker, so the gem's per-unpacker stack guard resets
  # at each level and the ext-envelope recursion is unbounded on the Ruby
  # call stack — deep enough input would exhaust the stack and escape the
  # codec's rescue (SystemStackError is not a Codec::Error). Over-deep
  # ext nesting must surface as a clean wire violation, matching the
  # nesting-depth guarantee for plain Array / Hash payloads.
  def test_over_deep_nested_fault_rejected
    assert_raises(InvalidType,
                  "ext 0x02 nested past the depth cap through #decode must raise InvalidType, not trap the stack") do
      Decoder.decode(nested_fault_bytes(200))
    end
  end

  def test_nested_fault_within_cap_round_trips
    decoded = Decoder.decode(nested_fault_bytes(8))
    assert_instance_of Kobako::Fault, decoded,
                       "a Fault chain within the nesting cap through #decode must decode to a Kobako::Fault"
  end

  # The depth counter is unwound by an ensure, so a rejected over-deep
  # chain must leave no residue that trips a later decode on the same
  # thread — otherwise one bad payload would poison every subsequent
  # invocation sharing that thread.
  def test_over_deep_rejection_leaves_no_depth_residue
    assert_raises(InvalidType) { Decoder.decode(nested_fault_bytes(200)) }
    decoded = Decoder.decode(nested_fault_bytes(8))
    assert_instance_of Kobako::Fault, decoded,
                       "a within-cap decode after a rejected over-deep chain must still succeed"
  end

  # A host that builds an over-deep Fault chain (e.g. a Service that wraps
  # its own faults without bound) must be refused at the encode boundary
  # rather than recurse until the stack overflows.
  def test_over_deep_nested_fault_rejected_at_encode
    deep = (1..200).reduce(Kobako::Fault.new(type: "runtime", message: "x")) do |inner, _|
      Kobako::Fault.new(type: "runtime", message: "x", details: inner)
    end
    assert_raises(UnsupportedType,
                  "an over-deep Fault chain through #encode must raise UnsupportedType, not trap the stack") do
      Encoder.encode(deep)
    end
  end

  private

  # Frame +payload_bytes+ (a msgpack map) as an ext 0x02 Fault envelope.
  # ext 32 (0xc9) keeps the length field wide enough for the growing
  # nested chain.
  def ext_fault(payload_bytes)
    [0xc9, payload_bytes.bytesize, 0x02].pack("CNC").b + payload_bytes
  end

  # A fixmap-3 { "type" => "runtime", "message" => "x", "details" => ... }
  # whose +details+ carries the already-encoded inner bytes.
  def fault_map(details_bytes)
    "\x83\xa4type\xa7runtime\xa7message\xa1x\xa7details".b + details_bytes
  end

  # Wire bytes for +depth+ ext 0x02 envelopes chained through +details+,
  # innermost +details+ being nil.
  def nested_fault_bytes(depth)
    depth.times.reduce("\xc0".b) { |inner, _| ext_fault(fault_map(inner)) }
  end
end
