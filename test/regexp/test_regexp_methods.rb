# frozen_string_literal: true

require "test_helper"

# Regexp instance- and class-method parity (SPEC.md B-41). Expected values
# are the C-gem (Onigmo) oracle harvested from data/kobako.wasm; #options
# numeric values diverge toward MRI and live in test_divergences.rb.
class TestRegexpMethods < Minitest::Test
  include RegexpParityHelper

  def test_match_operator_returns_byte_index
    assert_parity(2, '"ab12" =~ /\d+/',
                  "Regexp#=~ returns the byte index of the first match")
  end

  def test_match_operator_returns_nil_when_no_match
    assert_parity_nil('"abc" =~ /\d+/',
                      "Regexp#=~ returns nil, not -1 or false, when there is no match")
  end

  def test_match_predicate_true_on_hit
    assert_parity(true, '/\d/.match?("a1")',
                  "Regexp#match? reports whether the pattern matches")
  end

  def test_case_equality_true_on_hit
    assert_parity(true, '/\d/ === "a1"',
                  "Regexp#=== is true when the pattern matches (case/when use)")
  end

  def test_case_equality_false_on_miss
    assert_parity(false, '/\d/ === "abc"',
                  "Regexp#=== is false when the pattern does not match")
  end

  def test_source_returns_pattern_text
    assert_parity("a.b", "/a.b/.source",
                  "Regexp#source returns the original pattern text")
  end

  def test_casefold_true_with_ignorecase
    assert_parity(true, "/x/i.casefold?",
                  "Regexp#casefold? is true for an /i pattern")
  end

  def test_casefold_false_without_ignorecase
    assert_parity(false, "/x/.casefold?",
                  "Regexp#casefold? is false for a pattern without /i")
  end

  def test_inspect_renders_literal_form
    assert_parity("/a.b/i", "/a.b/i.inspect",
                  "Regexp#inspect renders the /source/flags literal form")
  end

  def test_to_s_renders_inline_flag_group
    assert_parity("(?-mix:a.b)", "/a.b/.to_s",
                  "Regexp#to_s renders the (?-mix:source) inline-flag form")
  end

  def test_escape_quotes_metacharacters
    assert_parity('a\.b\*c\+d', 'Regexp.escape("a.b*c+d")',
                  "Regexp.escape backslash-quotes regexp metacharacters")
  end

  def test_compile_is_new_and_matches
    assert_parity("aaa", 'Regexp.compile("a+").match("baaa")[0]',
                  "Regexp.compile compiles a pattern like Regexp.new")
  end

  def test_runtime_new_round_trips_capture
    assert_parity("bbb", 'Regexp.new("a(b+)c").match("xabbbcx")[1]',
                  "Regexp.new compiles a runtime pattern and yields its capture")
  end

  def test_new_with_ignorecase_flag_matches_case_insensitively
    assert_parity("y", 'Regexp.new("ab", Regexp::IGNORECASE).match("AB") ? "y" : "n"',
                  "Regexp.new honours the Regexp::IGNORECASE option")
  end
end
