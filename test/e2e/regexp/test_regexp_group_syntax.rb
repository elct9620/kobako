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

  # CRuby answers ["ab ", "a"] and ["a", "a"] here: its parser keeps `#` a
  # comment after the group closes, though the option itself has ended.
  # @behavior RX-313
  def test_an_isolated_extended_option_ends_with_its_group_comments_included
    assert_equal [["ab # c", "a", "c"], %w[a#b a b]],
                 eval_regexp('[Regexp.new("((?x)a # (\n)b # (c)").match("ab # c").to_a, ' \
                             'Regexp.new("((?x)a)#(b)").match("a#b").to_a]'),
                 "a hash sign after a group holding (?x) through a match must read as literal text"
  end

  # @behavior RX-311
  def test_a_backreference_level_without_recursion_reads_one_level
    assert_equal [%w[aa a], %w[aa a], nil, ["b", nil]],
                 eval_regexp('[/(?<n>a)\k<n+0>/.match("aa"), /(a)\k<-1+0>/.match("aa"), ' \
                             '/(?<n>a)\k<n+1>/.match("aa"), /(a)\k<1-1>|b/.match("b")].map { |m| m && m.to_a }'),
                 "a backreference at level zero through a match must read its group, any other level nothing"
  end

  # @behavior RX-312
  def test_a_backreference_level_beside_a_call_or_a_shared_name_is_refused
    ['(?<n>a)\\\\k<n+0>\\\\g<n>', '(?<n>a)(?<n>b)\\\\k<n+0>'].each do |source|
      assert_equal "RegexpError", guard_error(%(Regexp.new("#{source}")), "RegexpError"),
                   "#{source} through Regexp.new must raise RegexpError"
    end
  end
end
