# frozen_string_literal: true

require "test_helper"

# The MatchData readers that answer about the match as a whole: its text,
# its size under either name, the subject it came from, and its copies.
class TestRegexpMatchDataReaders < Minitest::Test
  include RegexpGuestHelper

  MATCH = 'm = /(b)(c)/.match("abcd")'

  # @behavior RX-202
  def test_to_s_answers_the_whole_match
    assert_equal "bc", eval_regexp("#{MATCH}; m.to_s"),
                 "MatchData#to_s must answer the whole match"
  end

  # @behavior RX-203
  def test_length_counts_as_size_does
    assert_equal [3, 3], eval_regexp("#{MATCH}; [m.length, m.size]"),
                 "MatchData#length must count the same as #size"
  end

  # @behavior RX-213
  def test_regexp_answers_the_pattern_that_matched
    assert_equal ["(b)(c)", 4], eval_regexp('m = /(b)(c)/m.match("abcd"); [m.regexp.source, m.regexp.options]'),
                 "MatchData#regexp must answer the pattern that matched, flags included"
  end

  # @behavior RX-204
  def test_string_answers_the_subject
    assert_equal "abcd", eval_regexp("#{MATCH}; m.string"),
                 "MatchData#string must answer the subject the pattern was matched against"
  end

  # @behavior RX-205
  def test_clone_carries_the_same_snapshot_as_dup
    assert_equal true, eval_regexp("#{MATCH}; c = m.clone; d = m.dup; c.to_a == d.to_a && c.offset(1) == d.offset(1)"),
                 "MatchData#clone must carry the same snapshot as #dup"
  end
end
