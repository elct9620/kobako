# frozen_string_literal: true

require "test_helper"

# Byte-level golden vectors (SPEC.md → Wire Codec). These vectors fix the
# encoder output to specific byte sequences so the suite acts as a real
# spec-compliance check, not a self-consistency check — any silent drift
# toward a wider tag form breaks the Rust guest codec's decoder
# expectations. Hex is shown without separators for compact equality.
class TestCodecGoldenVectors < Minitest::Test
  include CodecHelpers

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
    # [42] -> 0x91 0x2a (fixarray len=1, positive fixint 42).
    # Generic msgpack codec check.
    assert_bytes "912a", [42]
  end

  def test_golden_vector_symbol_empty
    # :"" -> ext 8 len=0 type=0x00 -> 0xc7 0x00 0x00
    assert_bytes "c70000", :""
  end

  def test_golden_vector_symbol_short_uses_narrowest_ext
    # :hello -> ext 8 len=5 type=0x00 followed by "hello" bytes.
    assert_bytes "c7050068656c6c6f", :hello
  end

  def test_golden_vector_handle
    # Handle(1) -> fixext4 ext 0x01 + big-endian u32 1
    # 0xd6 0x01 0x00 0x00 0x00 0x01
    assert_bytes "d60100000001", Handle.restore(1)
  end

  def test_golden_vector_handle_max
    # Handle(0x7fff_ffff) -> 0xd6 0x01 0x7f 0xff 0xff 0xff
    assert_bytes "d6017fffffff", Handle.restore(Handle::MAX_ID)
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

  # ---------- narrow zero-length tags (SPEC Wire Codec Type Mapping) ----------
  #
  # Each empty container must encode to its narrowest possible tag — the
  # single-byte "fix" form.

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

  # ---------- integer boundary tags (SPEC Wire Codec Type Mapping) ----------
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
end
