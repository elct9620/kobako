# frozen_string_literal: true

# Fast-tier E2E test for the wire ABI surface (SPEC item #9).
#
# Intentionally does NOT require "test_helper" — like the other clean-checkout
# tests, the ABI module must be exercisable without the native extension being
# compiled. We require lib/kobako/abi.rb directly.
#
# This test asserts the Ruby-side mirror of the Rust ABI declarations agrees
# with SPEC.md "ABI Signatures" and with the constants exported from
# wasm/kobako-wasm/src/abi.rs. Both sides derive the packed-u64 formula
# independently from SPEC; this test confirms they agree on representative
# (ptr, len) pairs.
require "minitest/autorun"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/abi"

class TestAbi < Minitest::Test
  def test_import_module_is_env
    # SPEC pins host import to the `env` wasm namespace
    # (REFERENCE Ch.5 wat-level shape).
    assert_equal "env", Kobako::ABI::IMPORT_MODULE
  end

  def test_import_name_matches_spec
    assert_equal "__kobako_rpc_call", Kobako::ABI::IMPORT_NAME
  end

  def test_exactly_three_export_names_in_spec_order
    assert_equal(
      %w[__kobako_run __kobako_alloc __kobako_take_outcome],
      Kobako::ABI::EXPORT_NAMES,
    )
    assert_equal 3, Kobako::ABI::EXPORT_NAMES.size,
                 "SPEC pins exactly 3 guest exports; no more, no less"
    assert Kobako::ABI::EXPORT_NAMES.frozen?
  end

  def test_pack_unpack_roundtrip_zero
    packed = Kobako::ABI.pack_u64(0, 0)
    assert_equal 0, packed
    assert_equal [0, 0], Kobako::ABI.unpack_u64(packed)
  end

  def test_pack_unpack_roundtrip_max
    packed = Kobako::ABI.pack_u64(0xffff_ffff, 0xffff_ffff)
    assert_equal 0xffff_ffff_ffff_ffff, packed
    assert_equal [0xffff_ffff, 0xffff_ffff], Kobako::ABI.unpack_u64(packed)
  end

  def test_pack_layout_is_high_ptr_low_len
    # SPEC ABI Signatures pins the bit layout: high 32 = ptr, low 32 = len.
    packed = Kobako::ABI.pack_u64(0xAABB_CCDD, 0x1122_3344)
    assert_equal 0xAABB_CCDD_1122_3344, packed
    assert_equal 0xAABB_CCDD, (packed >> 32) & 0xffff_ffff
    assert_equal 0x1122_3344, packed & 0xffff_ffff
  end

  def test_pack_unpack_roundtrip_common_pairs
    [
      [0x1000, 1024],
      [0x0001_0000, 4],
      [0x7fff_ffff, 0xffff],
      [1, 0xffff_ffff],
      [0xffff_ffff, 1],
    ].each do |ptr, len|
      packed = Kobako::ABI.pack_u64(ptr, len)
      assert_equal [ptr, len], Kobako::ABI.unpack_u64(packed),
                   "roundtrip failed for (#{ptr}, #{len})"
    end
  end

  def test_pack_rejects_out_of_range_ptr
    assert_raises(ArgumentError) { Kobako::ABI.pack_u64(-1, 0) }
    assert_raises(ArgumentError) { Kobako::ABI.pack_u64(1 << 32, 0) }
  end

  def test_pack_rejects_out_of_range_len
    assert_raises(ArgumentError) { Kobako::ABI.pack_u64(0, -1) }
    assert_raises(ArgumentError) { Kobako::ABI.pack_u64(0, 1 << 32) }
  end

  def test_unpack_rejects_out_of_range_packed
    assert_raises(ArgumentError) { Kobako::ABI.unpack_u64(-1) }
    assert_raises(ArgumentError) { Kobako::ABI.unpack_u64(1 << 64) }
  end

  # Cross-side derivation check: independently apply SPEC's bit-layout
  # formula and confirm Kobako::ABI.pack_u64 agrees on a known (ptr, len)
  # that the Rust side also asserts in `abi.rs` tests. Both sides derive
  # `(ptr << 32) | len` from SPEC ABI Signatures, so equal output here
  # is the contractual handshake item #12 (host linker) will rely on.
  def test_agrees_with_independent_spec_derivation
    ptr = 0xAABB_CCDD
    len = 0x1122_3344
    independently_derived = (ptr << 32) | len
    assert_equal independently_derived, Kobako::ABI.pack_u64(ptr, len)
  end
end
