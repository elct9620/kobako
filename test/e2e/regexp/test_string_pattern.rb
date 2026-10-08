# frozen_string_literal: true

require "test_helper"

# What the String methods that take a pattern accept as one. A dot
# discriminates a literal match from a compiled one: compiled, it would
# match every character.
class TestRegexpStringPattern < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-223
  def test_a_string_pattern_matches_its_own_characters
    assert_equal [["."], "a-c", "x-y-z"], eval_regexp('["a.c".scan("."), "a.c".sub(".", "-"), "x.y.z".gsub(".", "-")]'),
                 "a String pattern through String#scan, #sub and #gsub must match only its literal characters"
  end
end
