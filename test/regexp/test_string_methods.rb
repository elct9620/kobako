# frozen_string_literal: true

require "test_helper"

# String ⇄ Regexp integration contract (SPEC.md B-41). $1 inside a gsub
# block refreshes per iteration and lives in test_match_globals.rb.
class TestRegexpStringMethods < Minitest::Test
  include RegexpGuestHelper

  def test_match_returns_matchdata
    assert_equal %w[123 123], eval_regexp('"abc123".match(/(\d+)/).to_a'),
                 "String#match returns a MatchData populated with the captures"
  end

  def test_gsub_replaces_every_occurrence
    assert_equal "heLLo", eval_regexp('"hello".gsub(/l/, "L")'),
                 "String#gsub replaces every match with the replacement"
  end

  def test_gsub_with_block_uses_block_result
    assert_equal "a2b4", eval_regexp('"a1b2".gsub(/\d/){|m| (m.to_i * 2).to_s }'),
                 "String#gsub with a block substitutes each block result"
  end

  def test_sub_replaces_first_occurrence
    assert_equal "heLlo", eval_regexp('"hello".sub(/l/, "L")'),
                 "String#sub replaces only the first match"
  end

  def test_scan_collects_flat_matches
    assert_equal %w[1 2 3], eval_regexp('"a1b2c3".scan(/\d/)'),
                 "String#scan collects each match when the pattern has no groups"
  end

  def test_scan_collects_group_tuples
    assert_equal [%w[a 1], %w[b 2]], eval_regexp('"a1b2".scan(/([a-z])(\d)/)'),
                 "String#scan collects per-match group arrays when the pattern has groups"
  end

  def test_split_on_pattern
    assert_equal %w[a b c], eval_regexp('"a,b,c".split(/,/)'),
                 "String#split divides the string on each match"
  end

  def test_split_includes_capturing_groups
    assert_equal %w[a 1 b 2], eval_regexp('"a1b2".split(/(\d)/)'),
                 "String#split on a Regexp with a group interleaves each captured substring"
  end

  def test_split_with_positive_limit_caps_fields
    assert_equal ["a", "b,c,d"], eval_regexp('"a,b,c,d".split(/,/, 2)'),
                 "String#split with a positive limit stops splitting and keeps the remainder as the last field"
  end

  def test_split_on_pattern_with_negative_limit_keeps_trailing_empties
    assert_equal ["a", "b", "", ""], eval_regexp('"a,b,,".split(/,/, -1)'),
                 "String#split on a Regexp with a -1 limit keeps trailing empty fields"
  end

  def test_index_returns_byte_offset
    assert_equal 2, eval_regexp('"hello".index(/l/)'),
                 "String#index returns the byte offset of the first match"
  end

  def test_slice_returns_matched_substring
    assert_equal "ll", eval_regexp('"hello"[/l+/]'),
                 "String#[] with a Regexp returns the matched substring"
  end

  def test_slice_with_group_index_returns_capture
    assert_equal "2", eval_regexp('"a12b"[/(\d)(\d)/, 2]'),
                 "String#[] with a Regexp and group index returns that capture"
  end

  def test_sub_with_block_uses_block_result
    assert_equal "a9b2", eval_regexp('"a1b2".sub(/\d/){|m| (m.to_i * 9).to_s }'),
                 "String#sub with a block substitutes the first match's block result"
  end

  def test_scan_with_block_yields_each_match
    assert_equal %w[1 2], eval_regexp('r = []; "a1b2".scan(/\d/){|m| r << m }; r'),
                 "String#scan with a block yields each match to the block"
  end

  # Overriding []/index/split must preserve their core behaviour for a
  # non-Regexp argument: the override aliases the core method and delegates
  # to it. These pin that delegation so a regression in the alias wiring
  # cannot pass unnoticed.

  def test_split_on_string_delegates_to_core
    assert_equal %w[a b c], eval_regexp('"a,b,c".split(",")'),
                 "String#split with a String argument delegates to the core method"
  end

  def test_split_with_negative_limit_keeps_trailing_empties
    assert_equal ["a", "b", "c", "", ""], eval_regexp('"a,b,c,,".split(",", -1)'),
                 "String#split with a -1 limit keeps trailing empty fields via the core method"
  end

  def test_index_of_string_delegates_to_core
    assert_equal 2, eval_regexp('"hello".index("l")'),
                 "String#index with a String argument delegates to the core method"
  end

  def test_aref_with_string_delegates_to_core
    assert_equal "ell", eval_regexp('"hello"["ell"]'),
                 "String#[] with a String argument delegates to the core method"
  end

  def test_aref_with_integer_range_delegates_to_core
    assert_equal "ell", eval_regexp('"hello"[1, 3]'),
                 "String#[] with Integer arguments delegates to the core method"
  end
end
