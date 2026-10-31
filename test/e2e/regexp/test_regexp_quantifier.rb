# frozen_string_literal: true

require "test_helper"

# Intervals, where Ruby's reading differs from the engine's. Each expected value
# is MRI's answer to the same guest code.
class TestRegexpQuantifier < Minitest::Test
  include RegexpGuestHelper

  # A plus after an interval repeats it in Ruby, so "aaa" is taken whole where a
  # possessive reading would stop at two.
  # @behavior RX-284
  def test_a_quantifier_after_an_interval_repeats_it
    {
      '/a{1,2}+/.match("aaa")' => %w[aaa], '/a{2}+/.match("aaaa")' => %w[aaaa],
      '/a{1,2}*/.match("aaa")' => %w[aaa], '/a{1,2}{2}/.match("aaa")' => %w[aaa],
      '/a{1,2}+?/.match("aaa")' => %w[aa], '/a{1,2}++/.match("aaa")' => %w[aaa]
    }.each do |code, expected|
      assert_equal expected, eval_regexp("#{code}.to_a"), "#{code} must repeat the whole interval"
    end
  end

  # The repeat must reach back over the whole unit, whichever kind it is.
  # @behavior RX-284
  def test_a_quantifier_after_an_interval_repeats_every_kind_of_unit
    {
      '/(ab){1,2}+/.match("ababab")' => %w[ababab ab], '/[ab]{2}+/.match("abab")' => %w[abab],
      '/\d{1,2}+/.match("123")' => %w[123], '/é{1}+/.match("éé")' => %w[éé],
      '/(?:a|b){2}+/.match("abab")' => %w[abab]
    }.each do |code, expected|
      assert_equal expected, eval_regexp("#{code}.to_a"), "#{code} must repeat its whole unit"
    end
  end

  # @behavior RX-284
  def test_extended_whitespace_does_not_separate_an_interval_from_its_quantifier
    assert_equal %w[aaaa], eval_regexp('/a {2}+/x.match("aaaa").to_a'),
                 "whitespace before an interval through /x must not detach it from its unit"
    assert_equal %w[aaaa], eval_regexp('/a{2} +/x.match("aaaa").to_a'),
                 "whitespace after an interval through /x must not detach the next quantifier"
  end

  # @behavior RX-285
  def test_a_question_mark_after_an_interval_makes_it_lazy
    assert_equal %w[a], eval_regexp('/a{1,2}?/.match("aaa").to_a'),
                 "a{1,2}? through a match must take as little as it can"
    assert_equal %w[aa], eval_regexp('/a{2}?/.match("aaaa").to_a'),
                 "a{2}? through a match must still take its fixed count"
  end

  # @behavior RX-286
  def test_an_interval_with_no_lower_bound_starts_at_zero
    assert_equal %w[aa], eval_regexp('/a{,2}/.match("aaa").to_a'),
                 "a{,2} through a match must repeat up to its bound"
    assert_equal [""], eval_regexp('/a{,2}/.match("b").to_a'),
                 "a{,2} through a match must accept zero repeats"
  end

  # @behavior RX-287
  def test_a_brace_that_opens_no_interval_is_literal
    { "a{,}" => "a{,}", "a{x}" => "a{x}", "a{1" => "a{1", "a{1,2" => "a{1,2", "a{}" => "a{}", "a{ 1}" => "a{ 1}" }
      .each do |source, subject|
      assert_equal [subject], eval_regexp(%(Regexp.new("#{source}").match("#{subject}").to_a)),
                   "#{source} through Regexp.new must match its braces literally"
    end
  end

  # @behavior RX-287
  def test_a_brace_inside_a_class_or_after_an_escape_stays_itself
    assert_equal %w[+], eval_regexp('/[a{1,2}+]/.match("+").to_a'),
                 "an interval spelled in a class through a match must read as class members"
    assert_equal %w[{1,2}], eval_regexp('/\{1,2}+/.match("{1,2}").to_a'),
                 "an escaped brace through a match must read as a literal"
  end

  # A brace an escape owns opens no interval, and the escape is one unit.
  # @behavior RX-287
  def test_a_brace_an_escape_owns_stays_with_it
    assert_equal %w[a], eval_regexp('/\p{Alpha}/.match("a").to_a'),
                 "a property class through a match must keep its brace"
    assert_equal %w[1], eval_regexp('/\P{Alpha}/.match("1").to_a'),
                 "a negated property class through a match must keep its brace"
    assert_equal %w[abc], eval_regexp('/\p{Alpha}{1,2}+/.match("abc").to_a'),
                 "an interval after a property class through a match must repeat the whole class"
    assert_equal %w[A], eval_regexp('/\u{41}/.match("A").to_a'),
                 "a code point escape through a match must keep its brace"
  end
end
