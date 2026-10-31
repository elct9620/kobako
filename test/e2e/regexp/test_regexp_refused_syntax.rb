# frozen_string_literal: true

require "test_helper"

# Syntax MRI refuses that the engine would answer. Each case pairs with an
# accepted neighbour, so a blanket refusal cannot pass.
class TestRegexpRefusedSyntax < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-272
  def test_a_group_option_the_language_does_not_name_is_refused
    ["(?P<n>a)", "(?P=n)", "(?P>n)", "(?s)a", "(?s:a)", "(?U)a", "(?R)", "(?-a)a", "(?-u)a", "(?1)"].each do |source|
      assert_refused source
    end
  end

  # @behavior RX-272
  def test_a_group_option_the_language_names_still_reads
    { "(?i)A" => "a", "(?-i)a" => "a", "(?mix)a" => "a", "(?u)a" => "a", "(?i-mx:A)" => "a" }.each do |source, subject|
      assert_equal [subject], eval_regexp(%(Regexp.new("#{source}").match("#{subject}").to_a)),
                   "#{source} through Regexp.new must still read as an option"
    end
  end

  # @behavior RX-288
  def test_a_look_behind_refuses_a_backreference
    ['(a)(?<=\\\\1)', '(a)(?<!\\\\1)b', '(?<n>a)(?<=\\\\k<n>)', '(a)(?=(?<=\\\\1))',
     '(a)(?<=(?=\\\\1)a)'].each do |source|
      assert_refused source
    end
  end

  # @behavior RX-288
  def test_a_look_behind_refuses_a_look_ahead_or_an_atomic_group
    ["(?<=(?=a))a", "(?<=(?!b))a", "(?<=(?>a))b", "(?<!(?=a))b"].each { |source| assert_refused source }
  end

  # @behavior RX-288
  def test_a_look_behind_still_holds_what_the_language_allows
    {
      "(?<=a|bc)d" => [%w[bcd], %w[d]], "(?<=(?<=a)b)c" => [%w[abc], %w[c]], "(?<=(?<!a)b)c" => [%w[bc], %w[c]],
      "(?<=(a))b" => [%w[ab], %w[b a]], "(?<=(?i:A))b" => [%w[ab], %w[b]], "(a)(?=\\\\1)" => [%w[aa], %w[a a]]
    }.each do |source, ((subject), expected)|
      assert_equal expected, eval_regexp(%(Regexp.new("#{source}").match("#{subject}").to_a)),
                   "#{source} through Regexp.new must still answer"
    end
  end

  # @behavior RX-289
  def test_a_negative_look_behind_refuses_a_capture
    ["(?<!(a))b", "(?<!(?<n>a))b", "(?<=(?<!(a))b)c"].each { |source| assert_refused source }
  end

  # An unnamed group beside a named one captures nothing, so it may stay.
  # @behavior RX-289
  def test_a_negative_look_behind_still_holds_what_does_not_capture
    assert_equal %w[b], eval_regexp('/(?<!(?:a))b/.match("b").to_a'),
                 "a non-capturing group in a negative look-behind through a match must still answer"
    assert_equal %w[xb x], eval_regexp('/(?<n>x)(?<!(a))b/.match("xb").to_a'),
                 "an unnamed group beside a name in a negative look-behind through a match must still answer"
  end

  # @behavior RX-290
  def test_a_group_name_opening_with_a_digit_is_refused
    ["(?<1a>x)", "(?<1>x)", "(?'1a'x)"].each { |source| assert_refused source }
    assert_equal %w[x x], eval_regexp('/(?<a1>x)/.match("x").to_a'),
                 "a group name holding a later digit through a match must still read"
  end

  # @behavior RX-291
  def test_an_interval_whose_upper_bound_is_below_its_lower_is_refused
    ["a{2,1}", "a{1,0}", "a{3,2}+", "a{1,2}{2,1}"].each { |source| assert_refused source }
    assert_equal [""], eval_regexp('/a{0,0}/.match("a").to_a'),
                 "an interval with equal bounds through a match must still read"
  end

  private

  def assert_refused(source)
    assert_equal "RegexpError", guard_error(%(Regexp.new("#{source}").match("a")), "RegexpError"),
                 "#{source} through Regexp.new must raise RegexpError"
  end
end
