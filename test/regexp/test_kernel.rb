# frozen_string_literal: true

require "test_helper"

# Kernel#=~ fallback (SPEC.md B-41). String defines its own regexp-aware =~;
# every other receiver falls through to Kernel#=~, which the C Onigmo gem
# fixed at nil (MRI's deprecated Object#=~).
class TestRegexpKernel < Minitest::Test
  include RegexpGuestHelper

  def test_integer_match_operator_returns_nil
    assert_nil eval_regexp("42 =~ /4/"),
               "a non-String receiver's =~ returns nil through the Kernel fallback"
  end

  def test_symbol_match_operator_returns_nil
    assert_nil eval_regexp(":sym =~ /s/"),
               "a Symbol's =~ returns nil through the Kernel fallback"
  end

  def test_string_match_operator_still_matches
    assert_equal 2, eval_regexp('"ab12" =~ /\d/'),
                 "String#=~ still overrides the Kernel fallback"
  end

  # MRI's String#=~ rejects a String operand (a literal is not a pattern) and
  # dispatches any other operand to its own =~ (which falls to Kernel#=~).
  def test_string_match_operator_with_string_raises_type_error
    assert_equal "TypeError",
                 eval_regexp('begin; "x" =~ "y"; "no-error"; ' \
                             "rescue TypeError; 'TypeError'; rescue => e; e.class.to_s; end"),
                 "String#=~ with a String operand raises TypeError"
  end

  def test_string_match_operator_with_other_operand_returns_nil
    assert_nil eval_regexp('"x" =~ 5'),
               "String#=~ dispatches a non-String/Regexp operand to its own =~ (nil)"
  end
end
