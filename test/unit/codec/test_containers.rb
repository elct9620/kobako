# frozen_string_literal: true

require "test_helper"

# Payload-codec container round-trips (docs/wire/payload-msgpack.md
# § Type Mapping #7-#8): array / map across their length-tag boundaries,
# mixed and nested element fidelity, and the structural nesting depth guard.
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
  def test_over_deep_nesting_decodes_as_a_catchable_wire_violation
    # 1000 nested single-element arrays terminated by nil — far beyond the
    # ecosystem bound, well within the 16 MiB payload cap.
    over_deep = ("\x91".b * 1000) + "\xc0".b

    error = assert_raises(InvalidTypeError) { Decoder.decode(over_deep) }

    assert_kind_of StandardError, error,
                   "an over-deep wire value must surface as a catchable wire violation, " \
                   "not a host SystemStackError the dispatcher's rescue StandardError would miss"
  end

  # The encode-side twin of the guard above. The msgpack gem bounds its
  # unpacker's recursion but not its packer's, so a value that nests
  # without bound leaves the packer as a Ruby SystemStackError — outside
  # the StandardError family every caller of this codec rescues. The
  # dispatch answer and the yield argument are both encoded here, so an
  # unmapped overflow escapes the dispatcher's boundary and traps the
  # invocation instead of reporting the Service's own value.
  #
  # It must not answer as UnsupportedTypeError either: that is the one
  # codec fault the dispatch answer path rescues into Handle allocation,
  # so an over-deep value routed there would be minted as an opaque Handle
  # rather than refused — the opposite of what the #run argument path does
  # with the same value (E-54).
  def test_cyclic_array_encodes_as_a_catchable_wire_violation
    cyclic = []
    cyclic << cyclic

    error = assert_raises(InvalidTypeError) { Encoder.encode(cyclic) }

    assert_kind_of StandardError, error,
                   "a self-referential Array through Encoder.encode must surface as a catchable " \
                   "wire violation, not a host SystemStackError the dispatcher's rescue " \
                   "StandardError would miss"
  end

  # The Hash half of the same shape is out of this codec's reach. msgpack
  # walks a Hash through `rb_hash_foreach`, whose C frames carry no Ruby
  # stack guard, so a cycle exhausts the machine stack and the interpreter
  # dies before any rescue here can run — with or without kobako in the
  # picture. Only refusing the value before the packer is handed it would
  # reach this case.
  def test_cyclic_hash_is_out_of_reach_of_a_host_side_mapping
    skip "a cyclic Hash kills the interpreter inside msgpack's rb_hash_foreach walk, so no " \
         "assertion about Encoder.encode can be made from inside this process"
  end

  # An acyclic tree deep enough to exhaust the packer refuses the same way
  # and is equally unwitnessable: a structure that deep outlives the example
  # and overflows the collector's own walk, taking the runner down during a
  # later test.

  # Characterization of the one asymmetry left in this codec. The msgpack
  # gem bounds its unpacker at the wire bound but offers no limit to set on
  # its packer, so this host writes a value its own reader refuses — and the
  # guest, whose encoder does carry the bound, refuses it too. Nothing
  # crosses that should not; the reporting side simply moves. Giving the
  # encoder the bound would change this test deliberately.
  def test_the_encoder_writes_past_the_bound_its_own_decoder_enforces
    past_bound = (1..(Kobako::Codec::MAX_NESTING_DEPTH + 1)).reduce([]) { |inner, _| [inner] }

    bytes = Encoder.encode(past_bound)

    assert_raises(InvalidTypeError) { Decoder.decode(bytes) }
  end

  def test_the_bound_the_decoder_enforces_is_the_wire_bound
    at_bound = (1..Kobako::Codec::MAX_NESTING_DEPTH).reduce([]) { |inner, _| [inner] }

    _, decoded = roundtrip(at_bound)

    assert_equal at_bound, decoded,
                 "a value nesting exactly to the wire bound must round-trip through this host's " \
                 "codec, so the depth this host refuses at is the documented one"
  end
end
