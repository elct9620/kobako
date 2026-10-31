# frozen_string_literal: true

require "test_helper"

# Options written inside a pattern, where Ruby's letters and scope differ from
# the engine's. Each expected value is MRI's answer to the same guest code.
class TestRegexpInlineOptions < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-280
  def test_an_inline_multiline_option_lets_a_dot_cross_a_newline
    ["(?m)a.b", "(?m:a.b)", "a(?m).b", "(?mix)A . B"].each do |source|
      assert_equal ["a\nb"], eval_regexp(%(Regexp.new("#{source}").match("a\\nb").to_a)),
                   "#{source} through Regexp.new must let a dot cross a newline"
    end
  end

  # @behavior RX-280
  def test_turning_multiline_off_inline_stops_a_dot_and_keeps_line_anchors
    assert_equal ["b"], eval_regexp('/(?-m)^b/.match("a\nb").to_a'),
                 "(?-m) through a match must leave ^ matching per line"
    assert_equal ["a"], eval_regexp('/(?-m:a$)/.match("a\nb").to_a'),
                 "(?-m: through a match must leave $ matching per line"
    assert_nil eval_regexp('/(?-m)a.b/m.match("a\nb")&.to_a'),
               "(?-m) through a match must stop a dot the /m flag let cross"
    assert_nil eval_regexp('/(?i-m)A.b/m.match("a\nb")&.to_a'),
               "(?i-m) through a match must stop a dot while it folds case"
  end

  # @behavior RX-281
  def test_an_inline_option_holds_through_the_alternatives_after_it
    assert_nil eval_regexp('/a(?i)b|c/.match("c")&.to_a'),
               "a(?i)b|c through a match must require the a before every alternative"
    assert_equal ["aC"], eval_regexp('/a(?i)b|c/.match("aC").to_a'),
                 "a(?i)b|c through a match must fold case in every alternative after the option"
    assert_nil eval_regexp('/(a(?i)b|c)d/.match("cd")&.to_a'),
               "an inline option inside a group through a match must bind its group the same way"
  end

  # @behavior RX-281
  def test_an_inline_option_ends_with_its_group
    assert_equal ["a#b", "a#b", "b"], eval_regexp('Regexp.new("(?x)(a(?-x)#(b))#(c)").match("a#b").to_a'),
                 "an inline -x through Regexp.new must end with the group holding it"
    assert_equal %w[a], eval_regexp('Regexp.new("(?x)a # (").match("a").to_a'),
                 "a comment reaching the end through Regexp.new must not swallow what closes its option"
  end

  # @behavior RX-281
  def test_an_inline_option_may_turn_off_more_than_once
    assert_equal %w[a], eval_regexp('/(?-i-m)a/.match("a").to_a'),
                 "(?-i-m) through a match must read every letter as turned off"
    assert_equal %w[a], eval_regexp('/(?-)a/.match("a").to_a'),
                 "(?-) through a match must read as no change"
  end
end
