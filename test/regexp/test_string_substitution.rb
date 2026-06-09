# frozen_string_literal: true

require "test_helper"

# gsub / sub replacement-string semantics (SPEC.md B-41). A String
# replacement expands backreferences (\0..\9 and \k<name>) against each
# match; a Hash replacement looks each whole match up as a key. Block forms
# live in test_string_methods.rb / test_match_globals.rb.
class TestRegexpStringSubstitution < Minitest::Test
  include RegexpGuestHelper

  def test_gsub_expands_numbered_backreferences
    assert_equal "1a2b", eval_regexp('"a1b2".gsub(/([a-z])(\d)/, \'\2\1\')'),
                 "String#gsub expands numbered backreferences in the replacement string"
  end

  def test_gsub_expands_named_backreference
    assert_equal "a[1]", eval_regexp('"a1".gsub(/(?<c>\d)/, \'[\k<c>]\')'),
                 "String#gsub expands a \\k<name> named backreference in the replacement"
  end

  def test_sub_expands_backreferences
    assert_equal "world hello", eval_regexp('"hello world".sub(/(\w+) (\w+)/, \'\2 \1\')'),
                 "String#sub expands backreferences in the replacement string"
  end

  def test_gsub_with_hash_replacement
    assert_equal "h311o", eval_regexp('"hello".gsub(/[el]/, { "e" => "3", "l" => "1" })'),
                 "String#gsub with a Hash replacement substitutes each whole match's mapped value"
  end

  def test_gsub_keeps_unrecognised_escape_literally
    assert_equal "\\z", eval_regexp('"a".gsub(/a/, \'\z\')'),
                 "String#gsub keeps an unrecognised \\x escape as its two literal characters"
  end

  def test_gsub_undefined_named_backreference_raises_index_error
    assert_equal "IndexError",
                 eval_regexp('begin; "a1".gsub(/\d/, \'\k<x>\'); "expanded"; ' \
                             'rescue IndexError; "IndexError"; rescue => e; e.class.to_s; end'),
                 "an undefined \\k<name> backreference raises IndexError"
  end

  def test_gsub_malformed_named_backreference_raises_regexp_error
    assert_equal "RegexpError",
                 eval_regexp('begin; "a1".gsub(/\d/, \'\k\'); "expanded"; ' \
                             'rescue RegexpError; "RegexpError"; rescue => e; e.class.to_s; end'),
                 "a \\k not followed by <name> raises RegexpError"
  end

  def test_gsub_zero_backreference_inserts_whole_match
    assert_equal "[a]b", eval_regexp('"ab".gsub(/a/, \'[\0]\')'),
                 "the \\0 backreference inserts the whole match"
  end

  def test_gsub_replacement_argument_wins_over_block
    assert_equal "aX", eval_regexp('"a1".gsub(/\d/, "X"){ "Y" }'),
                 "a replacement argument takes precedence over a block, as MRI does"
  end
end
