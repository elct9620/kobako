# frozen_string_literal: true

require "test_helper"

# Regexp#match / #match? position argument, MRI-aligned: a
# negative pos counts back from the end (as String#index does); a pos outside
# 0..length yields no match; pos equal to the length allows an empty match.
class TestRegexpMatchPosition < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-042
  def test_match_with_position_starts_search_at_offset
    assert_equal %w[2], eval_regexp('/\d/.match("a1b2c3", 3).to_a'),
                 "Regexp#match starts searching at the given byte position"
  end

  # @behavior RX-043
  def test_match_returns_nil_when_position_past_end
    assert_equal "nil", eval_regexp('//.match("abc", 5) ? "matched" : "nil"'),
                 "Regexp#match returns nil when pos is past the end of the string"
  end

  # @behavior RX-044
  def test_match_negative_position_counts_from_end
    assert_equal 2, eval_regexp('/a/.match("aba", -1).begin(0)'),
                 "Regexp#match resolves a negative pos from the end of the string"
  end

  # @behavior RX-045
  def test_match_returns_nil_when_negative_position_before_start
    assert_nil eval_regexp('/a/.match("abc", -10)'),
               "Regexp#match returns nil when a negative pos falls before the start"
  end

  # @behavior RX-046
  def test_match_at_end_position_allows_empty_match
    assert_equal 3, eval_regexp('//.match("abc", 3).begin(0)'),
                 "Regexp#match allows pos equal to the length, matching empty at the end"
  end

  # Offsets are bytes, so a position can fall inside a character; the
  # search starts at that character rather than splitting it.
  # @behavior RX-189
  def test_match_from_inside_a_multibyte_character_starts_at_its_boundary
    assert_equal ["é"], eval_regexp('/./.match("aéb", 2).to_a'),
                 "Regexp#match from a byte position inside a multibyte character must start at " \
                 "that character's boundary"
  end

  # @behavior RX-047
  def test_match_p_returns_false_when_position_past_end
    assert_equal false, eval_regexp('//.match?("abc", 5)'),
                 "Regexp#match? returns false when pos is past the end of the string"
  end

  # The only "b" sits at byte 0, so a match from 1.9 finds it only if the
  # position is misread as 0.
  # @behavior RX-249
  def test_match_position_reads_as_an_integer
    refusals = ['/b/.match("ab", "1")', '/b/.match("ab", nil)', '/b/.match?("ab", "1")', '/b/.match?("ab", nil)']

    assert_equal [[true, false], ["TypeError"] * refusals.size],
                 [eval_regexp('[/b/.match("ba", 1.9).nil?, /b/.match?("ba", 1.9)]'),
                  refusals.map { |code| guard_error(code, "TypeError") }],
                 "Regexp#match and #match? from a Float position through Sandbox#eval must count its " \
                 "whole part, and from a String or nil position must raise TypeError"
  end

  # @behavior RX-255
  def test_a_nil_subject_still_reads_its_position
    calls = ['/a/.match(nil, "1")', '/a/.match?(nil, "1")']

    assert_equal ["TypeError"] * calls.size, calls.map { |code| guard_error(code, "TypeError") },
                 "Regexp#match and #match? of nil from a String position through Sandbox#eval must raise " \
                 "TypeError rather than answer for the nil subject"
  end
end
