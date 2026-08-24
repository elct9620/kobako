# frozen_string_literal: true

require "test_helper"

# MatchData accessor contract (SPEC.md B-41; docs/regexp.md RX-03). Match
# offsets and slices are byte-based.
class TestRegexpMatchData < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-095
  def test_to_a_lists_full_match_then_captures
    assert_equal %w[12 12], eval_regexp('/(\d+)/.match("ab12cd").to_a'),
                 "MatchData#to_a lists the full match followed by each capture"
  end

  # @behavior RX-096
  def test_index_by_symbol_name
    assert_equal "12", eval_regexp('/(?<y>\d+)/.match("ab12")[:y]'),
                 "MatchData#[] resolves a named capture by Symbol"
  end

  # @behavior RX-097
  def test_index_by_string_name
    assert_equal "12", eval_regexp('/(?<y>\d+)/.match("ab12")["y"]'),
                 "MatchData#[] resolves a named capture by String"
  end

  # @behavior RX-098
  def test_begin_is_byte_offset_of_match_start
    assert_equal 1, eval_regexp('/\d+/.match("a12b").begin(0)'),
                 "MatchData#begin(0) is the byte offset where the match starts"
  end

  # @behavior RX-099
  def test_end_is_byte_offset_after_match
    assert_equal 3, eval_regexp('/\d+/.match("a12b").end(0)'),
                 "MatchData#end(0) is the byte offset just past the match"
  end

  # @behavior RX-100
  def test_offset_pairs_begin_and_end
    assert_equal [1, 3], eval_regexp('/\d+/.match("a12b").offset(0)'),
                 "MatchData#offset(0) pairs the begin and end byte offsets"
  end

  # @behavior RX-101
  def test_pre_match_is_text_before
    assert_equal "a", eval_regexp('/\d+/.match("a12b").pre_match'),
                 "MatchData#pre_match is the substring before the match"
  end

  # @behavior RX-102
  def test_post_match_is_text_after
    assert_equal "b", eval_regexp('/\d+/.match("a12b").post_match'),
                 "MatchData#post_match is the substring after the match"
  end

  # @behavior RX-103
  def test_captures_lists_groups_without_full_match
    assert_equal %w[1 2], eval_regexp('/(\d)(\d)/.match("a12").captures'),
                 "MatchData#captures lists each group, excluding the full match"
  end

  # @behavior RX-104
  def test_named_captures_maps_name_to_value
    assert_equal({ "a" => "1", "b" => "2" },
                 eval_regexp('/(?<a>\d)(?<b>\d)/.match("12").named_captures'),
                 "MatchData#named_captures maps each name to its captured String")
  end

  # @behavior RX-105
  def test_names_lists_capture_names
    assert_equal %w[a b], eval_regexp('/(?<a>\d)(?<b>\d)/.match("12").names'),
                 "MatchData#names lists the named-capture names in order"
  end

  # @behavior RX-106
  def test_named_captures_symbolize_names_uses_symbol_keys
    assert_equal({ a: "1", b: "2" },
                 eval_regexp('/(?<a>\d)(?<b>\d)/.match("12")' \
                             ".named_captures(symbolize_names: true)"),
                 "MatchData#named_captures(symbolize_names: true) keys by Symbol")
  end

  # @behavior RX-107
  def test_size_counts_full_match_plus_groups
    assert_equal 3, eval_regexp('/(\d)(\d)/.match("12").size'),
                 "MatchData#size counts the full match plus each group"
  end

  # MatchData.new is not constructible — a MatchData only ever arises from a
  # match, never direct construction. Resolve the error inside the guest (a
  # returned MatchData would surface a host codec error regardless, so this
  # asserts the guest-visible NoMethodError rather than the wire failure).
  # @behavior RX-108
  def test_new_is_not_constructible
    assert_equal "NoMethodError", guard_error("MatchData.new", "NoMethodError"),
                 "MatchData.new raises NoMethodError instead of constructing"
  end
end
