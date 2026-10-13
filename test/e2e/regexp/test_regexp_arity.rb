# frozen_string_literal: true

require "test_helper"

# How many arguments building and matching a pattern take. Each call either
# omits the one argument always needed or adds one past the most taken, so
# neither can be answered as an ordinary pattern or miss.
class TestRegexpArity < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-247
  def test_construction_refuses_no_argument_and_a_third
    calls = ["Regexp.new", 'Regexp.new("a", nil, nil)', "Regexp.compile", 'Regexp.compile("a", nil, nil)']

    assert_equal ["ArgumentError"] * calls.size, calls.map { |code| guard_error(code, "ArgumentError") },
                 "Regexp.new and Regexp.compile given no argument or three through Sandbox#eval must " \
                 "raise ArgumentError"
  end

  # @behavior RX-248
  def test_match_refuses_no_subject_and_a_third_argument
    calls = ["/a/.match", '/a/.match("a", 0, 1)', "/a/.match?", '/a/.match?("a", 0, 1)']

    assert_equal ["ArgumentError"] * calls.size, calls.map { |code| guard_error(code, "ArgumentError") },
                 "Regexp#match and Regexp#match? given no subject or three arguments through Sandbox#eval " \
                 "must raise ArgumentError"
  end
end
