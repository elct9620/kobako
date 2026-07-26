# frozen_string_literal: true

require "test_helper"
require "objspace"

# The Result arm's decode cost floor.
#
# A large invocation value crosses the boundary once: decoding a Result
# hands back a view onto the outcome buffer rather than a second copy.
# Nothing else here would notice if that changed — the value compares
# equal either way, so only its footprint tells a view from a copy.
#
# The view holds only because a Result payload is the value alone; a value
# in any other payload position is copied (benchmark/README.md § Known
# caveats). ObjectSpace.memsize_of reports the bytes an object owns, so a
# view reads far below its own length while a copy reads at or above it.
class TestOutcomeResultSharing < Minitest::Test
  # Comfortably past the msgpack gem's reference threshold, and far enough
  # past it that an owned buffer cannot be mistaken for a view.
  LARGE = "x" * 65_536

  def test_a_large_result_value_decodes_without_copying_its_bytes
    value = Kobako::Outcome.reify(:result, Kobako::Codec::Encoder.encode(LARGE), nil)

    assert_equal LARGE, value
    assert_operator ObjectSpace.memsize_of(value), :<, LARGE.bytesize,
                    "a large Result value through Kobako::Outcome.reify must decode as a view onto " \
                    "the outcome buffer — owning its bytes means it was copied a second time on the way in"
  end
end
