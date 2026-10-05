# frozen_string_literal: true

require "test_helper"

# String methods that ask a pattern about the whole String: scanning it,
# asking whether it matches, and slicing by a pattern.
class TestRegexpStringScan < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-209
  def test_scan_resumes_after_each_match
    assert_equal %w[aa aa], eval_regexp('"aaaaa".scan(/aa/)'),
                 "String#scan must resume after each match, so the matches it collects never overlap"
  end

  # @behavior RX-210
  def test_scan_with_a_block_answers_the_string
    assert_equal "abc", eval_regexp('"abc".scan(/b/) { |_| nil }'),
                 "String#scan with a block must answer the String it scanned"
  end

  # @behavior RX-211
  def test_match_predicate_answers_true_or_false
    assert_equal [true, false], eval_regexp('["abc".match?(/b/), "abc".match?(/z/)]'),
                 "String#match? must answer true or false, as Regexp#match? does"
  end

  # @behavior RX-212
  def test_slice_answers_as_indexing_does_for_a_pattern
    assert_equal [%w[bc bc], [nil, nil]], eval_regexp('s = "abcd"; [[s.slice(/b./), s[/b./]], [s.slice(/z/), s[/z/]]]'),
                 "String#slice handed a pattern must answer as String#[] does"
  end
end
