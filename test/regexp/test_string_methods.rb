# frozen_string_literal: true

require "test_helper"

# String ⇄ Regexp integration parity (SPEC.md B-41). Expected values are
# the C-gem (Onigmo) oracle harvested from data/kobako.wasm. $1 inside a
# gsub block diverges toward MRI and lives in test_divergences.rb.
class TestRegexpStringMethods < Minitest::Test
  include RegexpParityHelper

  def test_match_returns_matchdata
    assert_parity(%w[123 123], '"abc123".match(/(\d+)/).to_a',
                  "String#match returns a MatchData populated with the captures")
  end

  def test_gsub_replaces_every_occurrence
    assert_parity("heLLo", '"hello".gsub(/l/, "L")',
                  "String#gsub replaces every match with the replacement")
  end

  def test_gsub_with_block_uses_block_result
    assert_parity("a2b4", '"a1b2".gsub(/\d/){|m| (m.to_i * 2).to_s }',
                  "String#gsub with a block substitutes each block result")
  end

  def test_sub_replaces_first_occurrence
    assert_parity("heLlo", '"hello".sub(/l/, "L")',
                  "String#sub replaces only the first match")
  end

  def test_scan_collects_flat_matches
    assert_parity(%w[1 2 3], '"a1b2c3".scan(/\d/)',
                  "String#scan collects each match when the pattern has no groups")
  end

  def test_scan_collects_group_tuples
    assert_parity([%w[a 1], %w[b 2]], '"a1b2".scan(/([a-z])(\d)/)',
                  "String#scan collects per-match group arrays when the pattern has groups")
  end

  def test_split_on_pattern
    assert_parity(%w[a b c], '"a,b,c".split(/,/)',
                  "String#split divides the string on each match")
  end

  def test_index_returns_byte_offset
    assert_parity(2, '"hello".index(/l/)',
                  "String#index returns the byte offset of the first match")
  end

  def test_slice_returns_matched_substring
    assert_parity("ll", '"hello"[/l+/]',
                  "String#[] with a Regexp returns the matched substring")
  end

  def test_slice_with_group_index_returns_capture
    assert_parity("2", '"a12b"[/(\d)(\d)/, 2]',
                  "String#[] with a Regexp and group index returns that capture")
  end

  def test_sub_with_block_uses_block_result
    assert_parity("a9b2", '"a1b2".sub(/\d/){|m| (m.to_i * 9).to_s }',
                  "String#sub with a block substitutes the first match's block result")
  end

  def test_scan_with_block_yields_each_match
    assert_parity(%w[1 2], 'r = []; "a1b2".scan(/\d/){|m| r << m }; r',
                  "String#scan with a block yields each match to the block")
  end

  # Overriding []/index/split must preserve their core behaviour for a
  # non-Regexp argument: the override aliases the core method and delegates
  # to it. These pin that delegation so a regression in the alias wiring
  # cannot pass unnoticed.

  def test_split_on_string_delegates_to_core
    assert_parity(%w[a b c], '"a,b,c".split(",")',
                  "String#split with a String argument delegates to the core method")
  end

  def test_index_of_string_delegates_to_core
    assert_parity(2, '"hello".index("l")',
                  "String#index with a String argument delegates to the core method")
  end

  def test_aref_with_string_delegates_to_core
    assert_parity("ell", '"hello"["ell"]',
                  "String#[] with a String argument delegates to the core method")
  end

  def test_aref_with_integer_range_delegates_to_core
    assert_parity("ell", '"hello"[1, 3]',
                  "String#[] with Integer arguments delegates to the core method")
  end
end
