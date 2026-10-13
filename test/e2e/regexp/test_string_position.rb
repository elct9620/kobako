# frozen_string_literal: true

require "test_helper"

# How a String's pattern search reads its position and a pattern split reads
# its limit: as the language reads an Integer, so a Float counts by its whole
# part and anything else is refused rather than read as zero.
class TestRegexpStringPosition < Minitest::Test
  include RegexpGuestHelper

  # 1.9 searches from byte 1 and limits the split to one field, where reading
  # it as zero would search from the start and split without a limit.
  # @behavior RX-250
  def test_search_position_and_split_limit_read_as_an_integer
    refusals = ['"ab".index(/b/, "1")', '"ab".index(/b/, nil)', '"a,b".split(/,/, "1")', '"a,b".split(/,/, nil)']

    assert_equal [[nil, ["a,b"]], ["TypeError"] * refusals.size],
                 [eval_regexp('["ab".index(/a/, 1.9), "a,b".split(/,/, 1.9)]'),
                  refusals.map { |code| guard_error(code, "TypeError") }],
                 "String#index from and String#split up to a Float through Sandbox#eval must count its " \
                 "whole part, and a String or nil must raise TypeError"
  end
end
