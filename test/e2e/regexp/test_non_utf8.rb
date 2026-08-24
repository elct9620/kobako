# frozen_string_literal: true

require "test_helper"

# docs/regexp.md RX-09: bytes that are not UTF-8, on every text a Regexp
# operation reads.
#
# The engine matches over `&str` and offsets index into one, so a subject,
# a pattern, or a replacement whose bytes are not UTF-8 has no text to be.
# Refusing is the only honest answer available: reading such a String
# through a render answers the empty string, and the two failures that
# produces are both silent and both wrong — an empty subject reports "no
# match" for every pattern, and an empty pattern matches everywhere.
class TestRegexpNonUtf8 < Minitest::Test
  include RegexpGuestHelper

  # 255 begins no UTF-8 sequence, so `255.chr` is a one-byte String that
  # cannot be text.
  NON_UTF8 = "255.chr"

  # @behavior RX-087
  def test_non_utf8_subject_is_refused_by_match
    assert_equal "ArgumentError", guard_error("#{NON_UTF8} =~ /key/", "ArgumentError"),
                 "a subject whose bytes are not UTF-8 through =~ must raise, not report no match"
  end

  # @behavior RX-088
  def test_non_utf8_subject_is_refused_by_substitution
    assert_equal "ArgumentError", guard_error("(#{NON_UTF8}).sub(/k/, \"K\")", "ArgumentError"),
                 "a subject whose bytes are not UTF-8 through String#sub must raise, not answer an empty String"
  end

  # @behavior RX-089
  def test_non_utf8_pattern_source_is_refused
    assert_equal "ArgumentError", guard_error("Regexp.new(#{NON_UTF8})", "ArgumentError"),
                 "a pattern source whose bytes are not UTF-8 through Regexp.new must raise, " \
                 "not compile a pattern that matches everywhere"
  end

  # @behavior RX-090
  def test_non_utf8_replacement_is_refused
    assert_equal "ArgumentError", guard_error("\"abc\".sub(/b/, #{NON_UTF8})", "ArgumentError"),
                 "a replacement whose bytes are not UTF-8 through String#sub must raise, " \
                 "not splice in an empty String"
  end

  # @behavior RX-091
  def test_non_utf8_escape_argument_is_refused
    assert_equal "ArgumentError", guard_error("Regexp.escape(#{NON_UTF8})", "ArgumentError"),
                 "an argument whose bytes are not UTF-8 through Regexp.escape must raise, " \
                 "not answer an empty pattern source"
  end

  # The refusal must not reach past the bytes that provoke it: ordinary
  # text still matches, so this is a boundary and not a regression.
  # @behavior RX-092
  def test_utf8_text_still_matches
    assert_equal 3, eval_regexp('"abckey" =~ /key/'),
                 "a UTF-8 subject through =~ must still report its byte offset"
  end
end
