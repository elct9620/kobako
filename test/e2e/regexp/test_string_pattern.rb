# frozen_string_literal: true

require "test_helper"

# What the String methods that take a pattern accept as one. A dot
# discriminates a literal match from a compiled one: compiled, it matches
# every character.
class TestRegexpStringPattern < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-223
  def test_a_string_pattern_matches_its_own_characters
    assert_equal [["."], "a-c", "x-y-z"], eval_regexp('["a.c".scan("."), "a.c".sub(".", "-"), "x.y.z".gsub(".", "-")]'),
                 "a String pattern through String#scan, #sub and #gsub must match only its literal characters"
  end

  # A Symbol is refused as well as an Integer: its name would read as
  # text, yet the language takes only a String as a literal pattern.
  # @behavior RX-224
  def test_a_value_that_is_neither_a_pattern_nor_a_string_is_refused
    %w[1 :a].each do |pattern|
      ["scan(#{pattern})", "sub(#{pattern}, '-')", "gsub(#{pattern}, '-')"].each do |call|
        assert_equal "TypeError", guard_error("'a'.#{call}", "TypeError"),
                     "#{pattern} as the pattern through String##{call} must raise TypeError"
      end
    end
  end

  # @behavior RX-225
  def test_matching_compiles_a_string_pattern
    assert_equal [true, "bc"], eval_regexp('["axc".match?("."), "abc".match("b.")[0]]'),
                 "a String pattern through String#match? and #match must compile as the pattern it spells"
  end
end
