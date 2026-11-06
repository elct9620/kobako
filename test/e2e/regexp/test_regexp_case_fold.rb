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
end
