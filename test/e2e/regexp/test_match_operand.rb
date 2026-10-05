# frozen_string_literal: true

require "test_helper"

# The match-family operand contract. A Regexp
# match takes a String or Symbol subject, treats nil as no match, and raises
# TypeError on anything else (=== rescues to false). For String#match /
# #match? the pattern must be a Regexp (a String is not coerced) — anything
# else is a TypeError.
class TestRegexpMatchOperand < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-020
  def test_match_predicate_raises_type_error_on_integer_subject
    assert_equal "TypeError", guard_error("/2/.match?(123)", "TypeError"),
                 "a non-String/Symbol subject through Regexp#match? must raise TypeError"
  end

  # @behavior RX-021
  def test_match_raises_type_error_on_integer_subject
    assert_equal "TypeError", guard_error("/2/.match(123)", "TypeError"),
                 "a non-String/Symbol subject through Regexp#match must raise TypeError"
  end

  # @behavior RX-022
  def test_match_operator_raises_type_error_on_integer_subject
    assert_equal "TypeError", guard_error("/2/ =~ 123", "TypeError"),
                 "a non-String/Symbol subject through Regexp#=~ must raise TypeError"
  end

  # @behavior RX-023
  def test_case_equality_is_false_on_integer_subject
    assert_equal false, eval_regexp("/2/ === 123"),
                 "a non-String/Symbol subject through Regexp#=== must rescue to false, not stringify"
  end

  # @behavior RX-024
  def test_match_is_nil_on_nil_subject
    assert_nil eval_regexp("/a?/.match(nil)"),
               "a nil subject through Regexp#match must be no match (nil), not an empty-string match"
  end

  # @behavior RX-025
  def test_match_predicate_is_false_on_nil_subject
    assert_equal false, eval_regexp("/a?/.match?(nil)"),
                 "a nil subject through Regexp#match? must be no match (false)"
  end

  # @behavior RX-026
  def test_case_equality_accepts_symbol_subject
    assert_equal true, eval_regexp("/sy/ === :sym"),
                 "a Symbol subject through Regexp#=== must coerce to its name and match"
  end

  # @behavior RX-027
  def test_match_accepts_regexp_pattern
    assert_equal %w[x9 x 9], eval_regexp('"wx9z".match(/([a-z])(\d)/).to_a'),
                 "a Regexp pattern through String#match must match and return its MatchData"
  end

  SYMBOL_SUBJECT = "[/b/.match(:abc).to_a, /b/.match?(:abc), /b/ =~ :abc, /b/ === :abc]"

  # @behavior RX-190
  def test_the_match_family_reads_a_symbol_subject_as_its_name
    assert_equal [%w[b], true, 1, true], eval_regexp(SYMBOL_SUBJECT),
                 "Regexp#match, #match? and #=~ must read a Symbol subject as its name, as #=== does"
  end

  # @behavior RX-191 RX-192
  def test_a_nil_subject_is_no_match
    assert_equal [nil, false], eval_regexp("[/b/ =~ nil, /b/ === nil]"),
                 "Regexp#=~ must answer nil and Regexp#=== must answer false for a nil subject"
  end

  # @behavior RX-193
  def test_string_match_raises_type_error_on_string_pattern_through_the_match_form
    assert_equal "TypeError", guard_error('"abc".match("b")', "TypeError"),
                 "a String pattern through String#match, not only #match?, must raise TypeError"
  end

  # @behavior RX-028
  def test_string_match_raises_type_error_on_string_pattern
    assert_equal "TypeError", guard_error('"axc".match?(".")', "TypeError"),
                 "a String pattern through String#match? must raise TypeError (not coerced, mirroring C)"
  end

  # @behavior RX-029
  def test_string_match_raises_type_error_on_integer_pattern
    assert_equal "TypeError", guard_error('"s".match?(123)', "TypeError"),
                 "a non-Regexp pattern through String#match? must raise TypeError"
  end
end
