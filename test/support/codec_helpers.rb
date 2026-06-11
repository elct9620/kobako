# frozen_string_literal: true

# Shared aliases and assertions for the wire-codec coverage classes under
# test/codec/ (SPEC.md F-09). The codec is pure Ruby and needs no native
# extension; test_helper's no-ext fallback loads the whole pure-Ruby tree,
# so these classes still run on a clean checkout.
module CodecHelpers
  Encoder         = Kobako::Codec::Encoder
  Decoder         = Kobako::Codec::Decoder
  Handle          = Kobako::Handle
  Exc             = Kobako::Fault
  Truncated       = Kobako::Codec::Truncated
  InvalidType     = Kobako::Codec::InvalidType
  InvalidEncoding = Kobako::Codec::InvalidEncoding
  UnsupportedType = Kobako::Codec::UnsupportedType

  def roundtrip(value)
    bytes = Encoder.encode(value)
    decoded = Decoder.decode(bytes)
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
end
