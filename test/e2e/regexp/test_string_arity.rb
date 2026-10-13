# frozen_string_literal: true

require "test_helper"

# How many arguments a String's pattern methods take. Each call either omits
# the pattern or adds one argument past the most taken, so neither can be
# answered as an ordinary miss.
class TestRegexpStringArity < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-246
  def test_matching_scanning_and_substituting_refuse_other_argument_counts
    calls = ['"a".match', '"a".match(/a/, 0, 1)', '"a".match?', '"a".match?(/a/, 0, 1)',
             '"a".scan', '"a".scan(/a/, 1)', '"a".gsub', '"a".gsub(/a/, "b", 1)',
             '"a".sub', '"a".sub(/a/, "b", 1)']

    assert_equal ["ArgumentError"] * calls.size, calls.map { |code| guard_error(code, "ArgumentError") },
                 "String#match, #match?, #scan, #gsub and #sub through Sandbox#eval must raise " \
                 "ArgumentError for a missing pattern or an extra argument"
  end

  # @behavior RX-251
  def test_searching_and_splitting_by_a_pattern_refuse_a_third_argument
    calls = ['"a".index(/a/, 0, 1)', '"a".split(/a/, 0, 1)']

    assert_equal ["ArgumentError"] * calls.size, calls.map { |code| guard_error(code, "ArgumentError") },
                 "String#index and String#split by a pattern with a third argument through Sandbox#eval " \
                 "must raise ArgumentError"
  end
end
