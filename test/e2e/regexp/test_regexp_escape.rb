# frozen_string_literal: true

require "test_helper"

# How a pattern reads an escape the engine spells differently or not at all.
class TestRegexpEscape < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-303
  def test_an_octal_escape_names_a_character
    assert_equal ["A", "A", "\n3", "a\b", "\u00018"],
                 eval_regexp('[/\101/, /[\101]/, /\0123/, /(a)\10/, /\18/].zip(["A", "A", "\n3", "a\b", "\u00018"])' \
                             ".map { |r, s| r.match(s)[0] }"),
                 "octal escapes, in and out of a class and past the groups, through a match must name their characters"
  end

  # The same two digits read as a backreference once that many groups exist.
  # @behavior RX-303
  def test_a_number_naming_a_group_stays_a_backreference
    assert_equal "abcdefghijj",
                 eval_regexp('/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)\10/.match("abcdefghijj")[0]'),
                 "\\10 after ten groups through a match must refer to the tenth group"
  end

  # @behavior RX-304
  def test_a_control_escape_names_a_control_character
    assert_equal ["\u0001", "\u0001", "\u001C", "a\u0001b"],
                 eval_regexp('[/\cA/, /[\C-a]/, /\c\\\\/, /a\cab/].zip(["\u0001", "\u0001", "\u001C", "a\u0001b"])' \
                             ".map { |r, s| r.match(s)[0] }"),
                 "control escapes through a match must name the character's low five bits"
  end

  # @behavior RX-305
  def test_an_escaped_letter_without_a_meaning_reads_as_itself
    assert_equal %w[oo g q Z 81],
                 eval_regexp('[/\o{2}/, /\g/, /\q/, /[\Z]/, /\81/].zip(%w[oo g q Z 81])' \
                             ".map { |r, s| r.match(s)[0] }"),
                 "an escaped letter MRI gives no meaning through a match must read as the letter"
  end

  # @behavior RX-306
  def test_a_braced_unicode_escape_may_name_several_characters
    assert_equal %w[ABB A], eval_regexp('[/\u{41 42}+/.match("ABB")[0], /[\u{42 41}]/.match("A")[0]]'),
                 "\\u{41 42} through a match must name both characters, in and out of a class"
  end

  # @behavior RX-307
  def test_a_meta_escape_is_refused
    assert_equal "RegexpError", guard_error('Regexp.new("\\\\M-a")', "RegexpError"),
                 "\\M-a through Regexp.new must raise RegexpError, since no UTF-8 pattern holds its byte"
  end
end
