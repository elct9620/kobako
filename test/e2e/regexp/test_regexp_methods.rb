# frozen_string_literal: true

require "test_helper"

# Regexp instance- and class-method contract (SPEC.md B-41). Offsets are
# byte-based and #options reports MRI's option bits; how a pattern that cannot
# compile or match cheaply surfaces is covered in test_pattern_errors.rb.
class TestRegexpMethods < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-001
  def test_match_operator_returns_byte_index
    assert_equal 2, eval_regexp('"ab12" =~ /\d+/'),
                 "Regexp#=~ returns the byte index of the first match"
  end

  # @behavior RX-002
  def test_match_operator_returns_nil_when_no_match
    assert_nil eval_regexp('"abc" =~ /\d+/'),
               "Regexp#=~ returns nil, not -1 or false, when there is no match"
  end

  # @behavior RX-003
  def test_match_predicate_true_on_hit
    assert_equal true, eval_regexp('/\d/.match?("a1")'),
                 "Regexp#match? reports whether the pattern matches"
  end

  # @behavior RX-004
  def test_case_equality_true_on_hit
    assert_equal true, eval_regexp('/\d/ === "a1"'),
                 "Regexp#=== is true when the pattern matches (case/when use)"
  end

  # @behavior RX-005
  def test_case_equality_false_on_miss
    assert_equal false, eval_regexp('/\d/ === "abc"'),
                 "Regexp#=== is false when the pattern does not match"
  end

  # @behavior RX-006
  def test_source_returns_pattern_text
    assert_equal "a.b", eval_regexp("/a.b/.source"),
                 "Regexp#source returns the original pattern text"
  end

  # @behavior RX-007
  def test_casefold_true_with_ignorecase
    assert_equal true, eval_regexp("/x/i.casefold?"),
                 "Regexp#casefold? is true for an /i pattern"
  end

  # @behavior RX-008
  def test_casefold_false_without_ignorecase
    assert_equal false, eval_regexp("/x/.casefold?"),
                 "Regexp#casefold? is false for a pattern without /i"
  end

  # @behavior RX-009 RX-010
  def test_escape_quotes_metacharacters
    assert_equal 'a\.b\*c\+d', eval_regexp('Regexp.escape("a.b*c+d")'),
                 "Regexp.escape backslash-quotes regexp metacharacters"
    assert_equal "a/b\\v", eval_regexp('Regexp.escape("a/b\v")'),
                 "Regexp.escape leaves a slash unescaped (MRI) while still quoting a vertical tab"
  end

  # @behavior RX-011
  def test_compile_is_new_and_matches
    assert_equal "aaa", eval_regexp('Regexp.compile("a+").match("baaa")[0]'),
                 "Regexp.compile compiles a pattern like Regexp.new"
  end

  # @behavior RX-012
  def test_runtime_new_round_trips_capture
    assert_equal "bbb", eval_regexp('Regexp.new("a(b+)c").match("xabbbcx")[1]'),
                 "Regexp.new compiles a runtime pattern and yields its capture"
  end

  # @behavior RX-013
  def test_new_with_ignorecase_flag_matches_case_insensitively
    assert_equal "y", eval_regexp('Regexp.new("ab", Regexp::IGNORECASE).match("AB") ? "y" : "n"'),
                 "Regexp.new honours the Regexp::IGNORECASE option"
  end

  # #options reports MRI's option bits (IGNORECASE = 1, MULTILINE = 4),
  # combined, rather than any engine-internal mask.
  # @behavior RX-014
  def test_options_reports_mri_ignorecase_bit
    assert_equal 1, eval_regexp("/x/i.options"),
                 "Regexp#options reports MRI's IGNORECASE bit (1)"
  end

  # @behavior RX-015
  def test_options_combines_mri_bits
    assert_equal 5, eval_regexp("/x/im.options"),
                 "Regexp#options combines MRI's IGNORECASE|MULTILINE bits (5)"
  end

  # #named_captures maps each capture name to the list of group numbers that
  # carry it (name => [index]); #names is its key list.
  # @behavior RX-016
  def test_named_captures_maps_names_to_group_numbers
    assert_equal({ "a" => [1], "b" => [2] },
                 eval_regexp("/(?<a>.)(?<b>.)/.named_captures"),
                 "Regexp#named_captures maps each name to its group numbers")
  end

  # @behavior RX-017
  def test_named_captures_is_empty_without_named_groups
    assert_equal({}, eval_regexp("/(.)(.)/.named_captures"),
                 "Regexp#named_captures is empty when no group is named")
  end

  # @behavior RX-018
  def test_names_lists_capture_names_in_declaration_order
    assert_equal %w[year month], eval_regexp("/(?<year>\\d+)-(?<month>\\d+)/.names"),
                 "Regexp#names lists the capture names in declaration order"
  end

  # @behavior RX-019
  def test_names_is_empty_without_named_groups
    assert_equal [], eval_regexp("/(.)(.)/.names"),
                 "Regexp#names is empty when no group is named"
  end
end
