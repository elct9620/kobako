# frozen_string_literal: true

require "test_helper"

# The match operator on each receiver, as CRuby defines it: String, Symbol and
# nil answer it, and any other receiver has none to answer with.
class TestRegexpMatchOperator < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-051
  def test_integer_match_operator_raises_no_method_error
    assert_equal "NoMethodError", guard_error("42 =~ /4/", "NoMethodError"),
                 "an Integer on the left of =~ through Sandbox#eval must raise NoMethodError, " \
                 "since no match operator is defined for it"
  end

  # The match sets $~ as String#=~ does, so the offset alone would not tell a
  # match from a coincidence.
  # @behavior RX-052
  def test_symbol_match_operator_matches_its_name
    assert_equal [1, "b"], eval_regexp("[:abc =~ /b/, $~[0]]"),
                 "a Symbol on the left of =~ through Sandbox#eval must answer the byte offset " \
                 "of the match in its name and set $~"
  end

  # @behavior RX-053
  def test_string_match_operator_still_matches
    assert_equal 2, eval_regexp('"ab12" =~ /\d/'),
                 "a String on the left of =~ through Sandbox#eval must answer the byte offset of the match"
  end

  # CRuby's String#=~ rejects a String operand (a literal is not a pattern)
  # and asks any other operand to match through its own =~.
  # @behavior RX-054
  def test_string_match_operator_with_string_raises_type_error
    assert_equal "TypeError", guard_error('"x" =~ "y"', "TypeError"),
                 "a String on the right of a String's =~ through Sandbox#eval must raise TypeError"
  end

  # @behavior RX-055
  def test_string_match_operator_with_integer_raises_no_method_error
    assert_equal "NoMethodError", guard_error('"x" =~ 5', "NoMethodError"),
                 "an Integer on the right of a String's =~ through Sandbox#eval must raise " \
                 "NoMethodError from the Integer it asks to match"
  end

  # nil keeps its own =~ in CRuby, so a String asking nil to match answers
  # nil as well.
  # @behavior RX-235
  def test_nil_match_operator_answers_nothing
    assert_equal [nil, nil], eval_regexp('[nil =~ /a/, "x" =~ nil]'),
                 "nil on either side of =~ through Sandbox#eval must answer nil"
  end

  # @behavior RX-236
  def test_symbol_match_operator_with_string_raises_type_error
    assert_equal "TypeError", guard_error(':abc =~ "b"', "TypeError"),
                 "a String on the right of a Symbol's =~ through Sandbox#eval must raise TypeError"
  end

  # The operator passes exactly one operand, so another count takes a send.
  # CRuby refuses that count rather than ignoring the extra operand.
  # @behavior RX-181
  def test_symbol_and_nil_match_operators_refuse_a_second_operand
    assert_equal %w[ArgumentError ArgumentError],
                 [guard_error(":abc.send(:=~, /b/, 3)", "ArgumentError"),
                  guard_error("nil.send(:=~, /b/, 3)", "ArgumentError")],
                 "a Symbol's and nil's =~ sent two operands through Sandbox#eval must raise " \
                 "ArgumentError rather than ignore the second"
  end
end
