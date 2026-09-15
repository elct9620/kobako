# frozen_string_literal: true

require "test_helper"

# Payload-codec container round-trips (docs/wire/payload-msgpack.md
# § Type Mapping #7-#8): array / map across their length-tag boundaries,
# mixed and nested element fidelity, and the structural nesting depth guard.
class TestCodecContainers < Minitest::Test
  include CodecHelpers

  # ---------- array ----------

  # @behavior WP-025
  def test_array_empty
    assert_roundtrip([])
  end

  # @behavior WP-026
  def test_array_mixed_types
    a = [nil, true, false, 1, -1, 1.5, "x", "y".b, [1, 2], { "k" => "v" }]
    assert_roundtrip(a)
  end

  # @behavior WP-027
  def test_array_nested
    assert_roundtrip([[[[[42]]]]])
  end

  # @behavior WP-028
  def test_array_crosses_array16_boundary
    [Array.new(15, 0), Array.new(16, 0), Array.new(0xffff, 0), Array.new(0x1_0000, 0)].each do |a|
      _, decoded = roundtrip(a)
      assert_equal a, decoded,
                   "a #{a.length}-element Array across the fixarray/array16/array32 tag boundaries " \
                   "must round-trip unchanged"
    end
  end

  # ---------- map ----------

  # @behavior WP-029
  def test_map_empty
    assert_roundtrip({})
  end

  # @behavior WP-030
  def test_map_string_keys
    assert_roundtrip({ "a" => 1, "b" => 2, "c" => nil })
  end

  # @behavior WP-031
  def test_map_non_string_keys
    # SPEC envelope rules forbid this in specific positions, but the
    # codec itself must handle arbitrary wire-legal keys.
    assert_roundtrip({ 1 => "one", 2 => "two", true => "t" })
  end

  # @behavior WP-032
  def test_map_nested
    assert_roundtrip({ "outer" => { "inner" => { "leaf" => [1, 2, 3] } } })
  end

  # ---------- deep nesting ----------

  # @behavior WP-033
  def test_deeply_nested_mixed
    h = Handle.restore(7)
    value = [
      { "handles" => [h, h], "names" => [:missing] },
      [{ "deep" => [{ "deeper" => [h] }] }]
    ]
    _, decoded = roundtrip(value)
    assert_equal value, decoded,
                 "a mixed tree of Handles and Symbols nested in Arrays and Hashes must round-trip unchanged"
  end

  # A structure nested beyond the codec's depth bound (the MessagePack
  # ecosystem's limit the host library enforces on decode —
  # docs/wire/payload-msgpack.md § Structural Nesting Depth) must surface as a clean
  # wire violation, never a Ruby SystemStackError or a host crash. The
  # guest→host dispatch path depends on this: the dispatcher rescues only
  # StandardError, so an over-deep guest request stays catchable solely
  # because the overflow is mapped into the InvalidTypeError taxonomy here.
  # @behavior WP-034
  def test_over_deep_nesting_decodes_as_a_catchable_wire_violation
    # 1000 nested single-element arrays terminated by nil — far beyond the
    # ecosystem bound, well within the 16 MiB payload cap.
    over_deep = ("\x91".b * 1000) + "\xc0".b

    error = assert_raises(InvalidTypeError) { Decoder.decode(over_deep) }

    assert_kind_of StandardError, error,
                   "an over-deep wire value must surface as a catchable wire violation, " \
                   "not a host SystemStackError the dispatcher's rescue StandardError would miss"
  end

  # Characterization of the one asymmetry left in this codec. The msgpack
  # gem bounds its unpacker at the wire bound but offers no limit to set on
  # its packer, so this host writes a value its own reader refuses. Nothing
  # crosses that should not: every position that hands this writer a value
  # bounds it first, and the guest's encoder carries the bound itself.
  # @behavior WP-036
  def test_the_encoder_writes_past_the_bound_its_own_decoder_enforces
    past_bound = (1..(Kobako::Codec::MAX_NESTING_DEPTH + 1)).reduce([]) { |inner, _| [inner] }

    bytes = Encoder.encode(past_bound)

    assert_raises(InvalidTypeError) { Decoder.decode(bytes) }
  end

  # @behavior WP-037
  def test_the_bound_the_decoder_enforces_is_the_wire_bound
    at_bound = (1..Kobako::Codec::MAX_NESTING_DEPTH).reduce([]) { |inner, _| [inner] }

    _, decoded = roundtrip(at_bound)

    assert_equal at_bound, decoded,
                 "a value nesting exactly to the wire bound must round-trip through this host's " \
                 "codec, so the depth this host refuses at is the documented one"
  end
end
