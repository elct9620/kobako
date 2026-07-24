# frozen_string_literal: true

require "test_helper"

# Wire-codec container round-trips (SPEC.md → Wire Codec → Type Mapping
# #7-#8): array / map across their length-tag boundaries, mixed and nested
# element fidelity, and the structural nesting depth guard.
class TestCodecContainers < Minitest::Test
  include CodecHelpers

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
      assert_equal a, decoded,
                   "a #{a.length}-element Array across the fixarray/array16/array32 tag boundaries " \
                   "must round-trip unchanged"
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

  # ---------- deep nesting ----------

  def test_deeply_nested_mixed
    h = Handle.restore(7)
    e = Exc.new(type: "undefined", message: "missing")
    value = [
      { "handles" => [h, h], "errors" => [e] },
      [{ "deep" => [{ "deeper" => [h] }] }]
    ]
    _, decoded = roundtrip(value)
    assert_equal value, decoded,
                 "a mixed tree of Handles and Faults nested in Arrays and Hashes must round-trip unchanged"
  end

  # A structure nested beyond the codec's depth bound (the MessagePack
  # ecosystem's limit the host library enforces on decode —
  # docs/wire-codec.md § Structural Nesting Depth) must surface as a clean
  # wire violation, never a Ruby SystemStackError or a host crash. The
  # guest→host dispatch path depends on this: the dispatcher rescues only
  # StandardError, so an over-deep guest request stays catchable solely
  # because the overflow is mapped into the InvalidType taxonomy here.
  def test_over_deep_nesting_decodes_as_a_catchable_wire_violation
    # 1000 nested single-element arrays terminated by nil — far beyond the
    # ecosystem bound, well within the 16 MiB payload cap.
    over_deep = ("\x91".b * 1000) + "\xc0".b

    error = assert_raises(InvalidType) { Decoder.decode(over_deep) }

    assert_kind_of StandardError, error,
                   "an over-deep wire value must surface as a catchable wire violation, " \
                   "not a host SystemStackError the dispatcher's rescue StandardError would miss"
  end
end
