# frozen_string_literal: true

require "test_helper"

# MatchData#begin / #end / #offset index handling (SPEC.md B-41): an index
# past the group count (or an undefined capture name) raises IndexError; a
# capture name resolves to its group; a valid-but-non-participating group is
# nil (MRI-aligned).
class TestRegexpMatchDataBounds < Minitest::Test
  include RegexpGuestHelper

  def test_begin_out_of_range_raises_index_error
    assert_equal "IndexError", guard_error('/(\d)/.match("a1").begin(5)', "IndexError"),
                 "MatchData#begin raises IndexError for an index past the group count"
  end

  def test_end_out_of_range_raises_index_error
    assert_equal "IndexError", guard_error('/(\d)/.match("a1").end(5)', "IndexError"),
                 "MatchData#end raises IndexError for an index past the group count"
  end

  def test_offset_out_of_range_raises_index_error
    assert_equal "IndexError", guard_error('/(\d)/.match("a1").offset(2)', "IndexError"),
                 "MatchData#offset raises IndexError for an index past the group count"
  end

  def test_begin_resolves_capture_name
    assert_equal 1, eval_regexp('/(?<y>\d)/.match("a1").begin(:y)'),
                 "MatchData#begin accepts a capture name and returns its byte offset"
  end

  def test_begin_of_non_participating_group_is_nil
    assert_nil eval_regexp('/(a)?(b)/.match("b").begin(1)'),
               "MatchData#begin is nil for a valid index whose group did not participate"
  end
end
