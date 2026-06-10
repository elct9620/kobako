# frozen_string_literal: true

require "test_helper"

# The match-family operand contract (docs/regexp.md RX-02 / RX-06). A Regexp
# match takes a String or Symbol subject, treats nil as no match, and raises
# TypeError on anything else (=== rescues to false). For String#match /
# #match? the pattern must be a Regexp (a String is not coerced) — anything
# else is a TypeError.
class TestMatchOperand < Minitest::Test
  include RegexpGuestHelper

  def test_match_predicate_raises_type_error_on_integer_subject
    assert_equal "TypeError", guard_error("/2/.match?(123)", "TypeError"),
                 "a non-String/Symbol subject through Regexp#match? must raise TypeError"
  end

  def test_match_raises_type_error_on_integer_subject
    assert_equal "TypeError", guard_error("/2/.match(123)", "TypeError"),
                 "a non-String/Symbol subject through Regexp#match must raise TypeError"
  end

  def test_match_operator_raises_type_error_on_integer_subject
    assert_equal "TypeError", guard_error("/2/ =~ 123", "TypeError"),
                 "a non-String/Symbol subject through Regexp#=~ must raise TypeError"
  end

  def test_case_equality_is_false_on_integer_subject
    assert_equal false, eval_regexp("/2/ === 123"),
                 "a non-String/Symbol subject through Regexp#=== must rescue to false, not stringify"
  end

  def test_match_is_nil_on_nil_subject
    assert_nil eval_regexp("/a?/.match(nil)"),
               "a nil subject through Regexp#match must be no match (nil), not an empty-string match"
  end

  def test_match_predicate_is_false_on_nil_subject
    assert_equal false, eval_regexp("/a?/.match?(nil)"),
                 "a nil subject through Regexp#match? must be no match (false)"
  end

  def test_case_equality_accepts_symbol_subject
    assert_equal true, eval_regexp("/sy/ === :sym"),
                 "a Symbol subject through Regexp#=== must coerce to its name and match"
  end

  def test_match_accepts_regexp_pattern
    assert_equal %w[123 123], eval_regexp('"abc123".match(/(\d+)/).to_a'),
                 "a Regexp pattern through String#match must match and return its MatchData"
  end

  def test_string_match_raises_type_error_on_string_pattern
    assert_equal "TypeError", guard_error('"axc".match?(".")', "TypeError"),
                 "a String pattern through String#match? must raise TypeError (not coerced, mirroring C)"
  end

  def test_string_match_raises_type_error_on_integer_pattern
    assert_equal "TypeError", guard_error('"s".match?(123)', "TypeError"),
                 "a non-Regexp pattern through String#match? must raise TypeError"
  end
end
