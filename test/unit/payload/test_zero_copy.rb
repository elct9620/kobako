# frozen_string_literal: true

require "test_helper"
require "objspace"

# The payload adapter's cost floor for a large value.
#
# The two-layer wire keeps payload decoding on the Ruby side partly
# because the MessagePack gem hands back a shared string for a large
# value — a view onto the buffer it was handed, not a copy of it — so a
# megabyte value crosses the boundary once rather than twice. Moving this
# decode into the native side would replace that view with a real copy,
# and the regression would be invisible to every other test here: the
# value compares equal either way.
#
# The sharing is a property of decoding a value on its own; a value
# nested inside a container document is copied out of the buffer. So this
# pins the floor where it exists rather than where a reader might assume
# it does.
#
# ObjectSpace.memsize_of reports the bytes an object owns, so a shared
# string reads far below its own length while a copy reads at or above
# it. That difference is what this measures.
class TestPayloadZeroCopy < Minitest::Test
  # Comfortably past the gem's shared-string threshold, and far enough
  # past it that an owned buffer cannot be mistaken for a view.
  LARGE = "x" * 65_536

  def test_a_large_value_decodes_without_copying_its_bytes
    decoded = Kobako::Codec::Decoder.decode(Kobako::Codec::Encoder.encode(LARGE))

    assert_equal LARGE, decoded
    assert_operator ObjectSpace.memsize_of(decoded), :<, LARGE.bytesize,
                    "a large value through the codec must decode as a view onto the wire buffer — " \
                    "owning its bytes means it was copied a second time on the way in"
  end
end
