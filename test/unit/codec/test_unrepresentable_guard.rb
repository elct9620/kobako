# frozen_string_literal: true

require "test_helper"

# The factory's BasicObject guard (ExtTypes#register_unrepresentable): a value
# outside the 12-entry type mapping is rejected as UnsupportedType rather than
# routed through msgpack's to_msgpack fallback. This makes the host's non-wire
# detection a positive allowlist, so a permissive method_missing object cannot
# answer the probe and mis-encode as nil — the host peer of the guest's
# classname allowlist and the Rust codec's closed Value enum.
class TestCodecUnrepresentableGuard < Minitest::Test
  include CodecHelpers

  def test_permissive_object_encodes_as_unsupported_not_nil
    perm = Object.new
    def perm.method_missing(name, *) = (name == :to_msgpack ? nil : super)
    def perm.respond_to_missing?(_name, _include_private = false) = true

    assert_raises(UnsupportedType,
                  "a permissive method_missing object through Encoder.encode must raise UnsupportedType, " \
                  "not answer the to_msgpack probe and mis-encode as nil") do
      Encoder.encode(perm)
    end
  end

  def test_basic_object_proxy_encodes_as_unsupported
    proxy = BasicObject.new
    def proxy.method_missing(_name, *) = nil

    assert_raises(UnsupportedType,
                  "a BasicObject proxy through Encoder.encode must raise UnsupportedType") do
      Encoder.encode(proxy)
    end
  end

  def test_guard_ext_id_is_rejected_on_decode
    bytes = [0xD4, 0x7F, 0x00].pack("C*") # fixext1 carrying the guard's inert id 0x7F
    assert_raises(InvalidType,
                  "an ext frame carrying the guard's packer-only id (0x7F) must be rejected as InvalidType") do
      Decoder.decode(bytes)
    end
  end
end
