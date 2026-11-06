# frozen_string_literal: true

require "test_helper"

# Case-insensitive matching where one character folds to several, as ß
# folds to "ss" and ﬃ to "ffi".
class TestRegexpCaseFold < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-314
  def test_case_insensitive_text_matches_across_a_multi_character_fold
    assert_equal [0, 0, 0, nil],
                 eval_regexp('["ß" =~ /ss/i, "SS" =~ /ß/i, "ﬃ" =~ /ffi/i, "ﬃ" =~ /ﬀi/i]'),
                 "case-insensitive text through =~ must match a multi-character fold of its own characters"
  end

  # @behavior RX-315
  def test_a_quantifier_takes_only_the_last_character_of_folded_text
    assert_equal [nil, "ßß"], eval_regexp('[/ss+/i.match("ß"), /(?:ss)+/i.match("ßß")].map { |m| m && m[0] }'),
                 "a quantifier after ss through a match must repeat only the last s"
  end

  # @behavior RX-316
  def test_a_case_insensitive_class_adds_the_fold_of_a_character_it_writes
    assert_equal ["ss", "s", nil, "ss"],
                 eval_regexp('[/[xß]/i, /[^ß]/i, /[à-ÿ]/i, /[ß-ÿ]/i].map { |r| m = r.match("ss"); m && m[0] }'),
                 "a case-insensitive class through a match must add the fold of a character it writes, not one it spans"
  end

  # ſ folds to s and the Kelvin sign to k, but CRuby folds no ASCII class
  # beyond ASCII; a literal letter still folds.
  # @behavior RX-317
  def test_a_case_insensitive_ascii_class_stays_within_ascii
    assert_equal [nil, nil, nil, 0, 0, 0],
                 eval_regexp('["ſ" =~ /\w/i, "K" =~ /\w/i, "ſ" =~ /[x\w]/i, ' \
                             '"ſ" =~ /[^\w]/i, "A" =~ /(?a)[[:lower:]]/i, "ſ" =~ /s/i]'),
                 "ſ and the Kelvin sign through a case-insensitive ASCII class must not match, but match its negation"
  end

  # @behavior RX-318
  def test_a_case_insensitive_negated_case_bracket_negates_before_it_folds
    assert_equal [0, 0, nil], eval_regexp('["a" =~ /[[:^lower:]]/i, "A" =~ /[[:^upper:]]/i, "ŉ" =~ /[[:^lower:]]/i]'),
                 "a letter through a case-insensitive negated case bracket must match when its other case lies outside"
  end

  # CRuby lets ß past [^ẞ]/i, though ẞ folds to ß.
  # @behavior RX-321
  def test_a_case_insensitive_negated_class_refuses_every_case_it_names
    assert_nil eval_regexp('"ß" =~ /[^ẞ]/i'),
               "ß through /[^ẞ]/i must not match"
  end

  # CRuby also matches sß against /ßs/i, though not ßs against /sß/i.
  # @behavior RX-322
  def test_case_insensitive_text_keeps_its_characters_in_order
    assert_equal [nil, 0], eval_regexp('["sß" =~ /ßs/i, "sss" =~ /ßs/i]'),
                 "sß through /ßs/i must not match, while a spelling in order does"
  end

  # CRuby refuses ß itself before a look-behind naming ß case-insensitively.
  # @behavior RX-323
  def test_a_case_insensitive_look_behind_matches_the_character_it_names
    assert_equal 2, eval_regexp('"ßa" =~ /(?<=ß)a/i'),
                 "ßa through /(?<=ß)a/i must match after the ß"
  end

  # CRuby leaves the Latin-1 letters out of these brackets' folds; the
  # guest folds them as it folds every other letter.
  # @behavior RX-319
  def test_a_case_insensitive_case_bracket_folds_latin1_letters_too
    assert_equal [0, 0], eval_regexp('["À" =~ /[[:lower:]]/i, "à" =~ /[[:upper:]]/i]'),
                 "a Latin-1 letter through a case-insensitive case bracket of the other case must match"
  end
end
