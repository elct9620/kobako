# frozen_string_literal: true

require "test_helper"

# The Codec.track_handles readout — the signal the dispatch paths use to
# skip the Handle-resolution walk when a decode carried no Capability
# Handle. The skip is a pure optimization, so an inverted or stale flag
# never corrupts a value; these tests pin the signal itself so a
# regression cannot silently re-enable the walk (flag stuck true) or
# leak a sighting across brackets. The same-thread reset case is the
# +carried_handle+ analogue of the depth-residue test in
# test_malformed.rb.
class TestCodecTrackHandles < Minitest::Test
  include CodecHelpers

  def test_handle_carrying_decode_reports_true
    bytes = Encoder.encode(["payload", Handle.restore(7)])
    value, carried = Kobako::Codec.track_handles { Decoder.decode(bytes) }
    assert carried,
           "a decode whose tree carries a Handle through Codec.track_handles must report carried_handle true"
    assert_equal ["payload", Handle.restore(7)], value,
                 "track_handles must return the block's decoded value unchanged"
  end

  def test_handle_free_decode_reports_false
    bytes = Encoder.encode(["payload", { "count" => 42 }])
    value, carried = Kobako::Codec.track_handles { Decoder.decode(bytes) }
    refute carried, "a Handle-free decode through Codec.track_handles must report carried_handle false"
    assert_equal ["payload", { "count" => 42 }], value,
                 "track_handles must return the block's decoded value unchanged"
  end

  def test_handle_sighting_does_not_leak_into_the_next_bracket
    handle_bytes = Encoder.encode(Handle.restore(7))
    plain_bytes = Encoder.encode("payload")
    _, first = Kobako::Codec.track_handles { Decoder.decode(handle_bytes) }
    _, second = Kobako::Codec.track_handles { Decoder.decode(plain_bytes) }
    assert first, "the Handle-carrying bracket must report carried_handle true"
    refute second,
           "a Handle-free bracket on the same thread must report false — the sighting resets per bracket"
  end
end
