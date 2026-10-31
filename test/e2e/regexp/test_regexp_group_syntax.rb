# frozen_string_literal: true

require "test_helper"

# How a pattern's source declares and refers to groups, where MRI's reading
# differs from the engine's. Each expected value is MRI's answer to the same
# guest code.
class TestRegexpGroupSyntax < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-270
  def test_an_unnamed_group_beside_a_named_one_captures_nothing
    assert_equal %w[ab a], eval_regexp('/(?<n>a)(b)/.match("ab").to_a'),
                 "an unnamed group through MatchData#to_a must not capture beside a named group"
    assert_equal 2, eval_regexp('/(?<n>a)(b)/.match("ab").size'),
                 "an unnamed group through MatchData#size must not count beside a named group"
    assert_equal %w[ab b], eval_regexp('/(a)(?<n>b)/.match("ab").to_a'),
                 "a named group through MatchData#to_a must take number one past an unnamed group before it"
    assert_equal "b", eval_regexp('/(a)(?<n>b)/.match("ab")[:n]'),
                 "a named group through MatchData#[] must read itself past an unnamed group before it"
  end

  # @behavior RX-270
  def test_a_pattern_with_no_names_still_captures_by_number
    assert_equal %w[ab a b], eval_regexp('/(a)(b)/.match("ab").to_a'),
                 "plain groups through MatchData#to_a must still capture when no group is named"
    assert_equal %w[aa a], eval_regexp('Regexp.new("(a)\\\\1").match("aa").to_a'),
                 "a numbered backreference must still work when no group is named"
  end

  # An inline x turns what follows it into a comment, so nothing spelled there
  # may read as a group, a reference or a refused form.
  # @behavior RX-273
  def test_an_inline_extended_comment_spells_nothing
    [
      ['(?x)(a) # (?<m>z)\n(b)', "ab", %w[ab a b]],
      ['(?x)(a)(b) # (?<m>z)\n\\\\1', "aba", %w[aba a b]],
      ['(?x)(?<n>a) # (?<m>z)\n(?<n>b)\\\\k<n>', "abb", %w[abb a b]],
      ['(?x)a # (?P<n>z)\n', "a", %w[a]],
      ['(?x:(?<n>a) # (?<m>z)\n)(b)', "ab", %w[ab a]]
    ].each do |source, subject, expected|
      assert_equal expected, eval_regexp(%(Regexp.new("#{source}").match("#{subject}").to_a)),
                   "#{source} through Regexp.new must treat its comment as a comment"
    end
  end

  # @behavior RX-273
  def test_an_inline_extended_option_ends_with_its_scope
    assert_equal ["a # z", "z"], eval_regexp('Regexp.new("(?x)a(?-x) # (?<m>z)").match("a # z").to_a'),
                 "a group after (?-x) through Regexp.new must count as a group"
    assert_equal %w[a#c c], eval_regexp('Regexp.new("(?x:a#(b)\n)#(c)").match("a#c").to_a'),
                 "a group after a closed (?x: through Regexp.new must count as a group"
  end

  # @behavior RX-271
  def test_a_reference_by_number_beside_a_named_group_is_refused
    ['(?<n>a)(b)\\\\1', '(?<n>a)\\\\k<1>', '(?<n>a)\\\\k<-1>', '(?<n>a)\\\\g<1>', "(?<n>a)(?(1)b)"].each do |source|
      assert_equal "RegexpError", guard_error(%(Regexp.new("#{source}")), "RegexpError"),
                   "#{source} through Regexp.new must raise RegexpError"
    end
  end

  # @behavior RX-272
  def test_a_python_style_group_is_refused
    ["(?P<n>a)", "(?P=n)", "(?P>n)"].each do |source|
      assert_equal "RegexpError", guard_error(%(Regexp.new("#{source}")), "RegexpError"),
                   "#{source} through Regexp.new must raise RegexpError"
    end
  end
end
