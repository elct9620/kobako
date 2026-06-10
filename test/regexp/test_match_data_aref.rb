# frozen_string_literal: true

require "test_helper"

# MatchData#[] argument forms beyond a single group index (SPEC.md B-41): a
# start+length or a Range slices the group list (whole match then captures); a
# negative index counts from the end; an undefined capture name raises.
class TestRegexpMatchDataAref < Minitest::Test
  include RegexpGuestHelper

  def test_aref_with_length_returns_subarray
    assert_equal %w[1 2], eval_regexp('/(\d)(\d)/.match("a12")[1, 2]'),
                 "MatchData#[] with a start and length returns that slice of the group list"
  end

  def test_aref_with_range_returns_subarray
    assert_equal %w[12 1], eval_regexp('/(\d)(\d)/.match("a12")[0..1]'),
                 "MatchData#[] with a Range returns that slice of the group list"
  end

  def test_aref_with_negative_index_counts_from_end
    assert_equal "2", eval_regexp('/(\d)(\d)/.match("a12")[-1]'),
                 "MatchData#[] with a negative index counts from the end of the group list"
  end

  def test_aref_with_undefined_name_raises_index_error
    assert_equal "IndexError", guard_error('/(\d)/.match("a1")[:nope]', "IndexError"),
                 "MatchData#[] with an undefined capture name raises IndexError"
  end
end
