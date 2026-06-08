# frozen_string_literal: true

require "test_helper"

# MatchData accessor parity (SPEC.md B-41). Match offsets and slices are
# byte-based, mirroring the curated regexp engine. Expected values are the
# C-gem (Onigmo) oracle harvested from data/kobako.wasm.
class TestRegexpMatchData < Minitest::Test
  include RegexpParityHelper

  def test_to_a_lists_full_match_then_captures
    assert_parity(%w[12 12], '/(\d+)/.match("ab12cd").to_a',
                  "MatchData#to_a lists the full match followed by each capture")
  end

  def test_index_by_symbol_name
    assert_parity("12", '/(?<y>\d+)/.match("ab12")[:y]',
                  "MatchData#[] resolves a named capture by Symbol")
  end

  def test_index_by_string_name
    assert_parity("12", '/(?<y>\d+)/.match("ab12")["y"]',
                  "MatchData#[] resolves a named capture by String")
  end

  def test_begin_is_byte_offset_of_match_start
    assert_parity(1, '/\d+/.match("a12b").begin(0)',
                  "MatchData#begin(0) is the byte offset where the match starts")
  end

  def test_end_is_byte_offset_after_match
    assert_parity(3, '/\d+/.match("a12b").end(0)',
                  "MatchData#end(0) is the byte offset just past the match")
  end

  def test_offset_pairs_begin_and_end
    assert_parity([1, 3], '/\d+/.match("a12b").offset(0)',
                  "MatchData#offset(0) pairs the begin and end byte offsets")
  end

  def test_pre_match_is_text_before
    assert_parity("a", '/\d+/.match("a12b").pre_match',
                  "MatchData#pre_match is the substring before the match")
  end

  def test_post_match_is_text_after
    assert_parity("b", '/\d+/.match("a12b").post_match',
                  "MatchData#post_match is the substring after the match")
  end

  def test_captures_lists_groups_without_full_match
    assert_parity(%w[1 2], '/(\d)(\d)/.match("a12").captures',
                  "MatchData#captures lists each group, excluding the full match")
  end

  def test_named_captures_maps_name_to_value
    assert_parity({ "a" => "1", "b" => "2" },
                  '/(?<a>\d)(?<b>\d)/.match("12").named_captures',
                  "MatchData#named_captures maps each name to its captured String")
  end

  def test_names_lists_capture_names
    assert_parity(%w[a b], '/(?<a>\d)(?<b>\d)/.match("12").names',
                  "MatchData#names lists the named-capture names in order")
  end

  def test_size_counts_full_match_plus_groups
    assert_parity(3, '/(\d)(\d)/.match("12").size',
                  "MatchData#size counts the full match plus each group")
  end

  def test_match_with_position_starts_search_at_offset
    assert_parity(%w[2], '/\d/.match("a1b2c3", 3).to_a',
                  "Regexp#match starts searching at the given byte position")
  end
end
