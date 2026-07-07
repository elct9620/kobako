# frozen_string_literal: true

require "test_helper"

# Regexp#match / #match? position argument (SPEC.md B-41), MRI-aligned: a
# negative pos counts back from the end (as String#index does); a pos outside
# 0..length yields no match; pos equal to the length allows an empty match.
class TestRegexpMatchPosition < Minitest::Test
  include RegexpGuestHelper

  def test_match_with_position_starts_search_at_offset
    assert_equal %w[2], eval_regexp('/\d/.match("a1b2c3", 3).to_a'),
                 "Regexp#match starts searching at the given byte position"
  end

  def test_match_returns_nil_when_position_past_end
    assert_equal "nil", eval_regexp('//.match("abc", 5) ? "matched" : "nil"'),
                 "Regexp#match returns nil when pos is past the end of the string"
  end

  def test_match_negative_position_counts_from_end
    assert_equal 2, eval_regexp('/a/.match("aba", -1).begin(0)'),
                 "Regexp#match resolves a negative pos from the end of the string"
  end

  def test_match_returns_nil_when_negative_position_before_start
    assert_nil eval_regexp('/a/.match("abc", -10)'),
               "Regexp#match returns nil when a negative pos falls before the start"
  end

  def test_match_at_end_position_allows_empty_match
    assert_equal 3, eval_regexp('//.match("abc", 3).begin(0)'),
                 "Regexp#match allows pos equal to the length, matching empty at the end"
  end

  def test_match_p_returns_false_when_position_past_end
    assert_equal false, eval_regexp('//.match?("abc", 5)'),
                 "Regexp#match? returns false when pos is past the end of the string"
  end
end
