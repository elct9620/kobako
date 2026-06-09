# frozen_string_literal: true

require "test_helper"

# Regexp instance- and class-method contract (SPEC.md B-41). Offsets are
# byte-based; #options reports MRI's option bits; an invalid pattern and a
# runaway backtracking pattern both surface as a guest RegexpError.
class TestRegexpMethods < Minitest::Test
  include RegexpGuestHelper

  def test_match_operator_returns_byte_index
    assert_equal 2, eval_regexp('"ab12" =~ /\d+/'),
                 "Regexp#=~ returns the byte index of the first match"
  end

  def test_match_operator_returns_nil_when_no_match
    assert_nil eval_regexp('"abc" =~ /\d+/'),
               "Regexp#=~ returns nil, not -1 or false, when there is no match"
  end

  def test_match_predicate_true_on_hit
    assert_equal true, eval_regexp('/\d/.match?("a1")'),
                 "Regexp#match? reports whether the pattern matches"
  end

  def test_case_equality_true_on_hit
    assert_equal true, eval_regexp('/\d/ === "a1"'),
                 "Regexp#=== is true when the pattern matches (case/when use)"
  end

  def test_case_equality_false_on_miss
    assert_equal false, eval_regexp('/\d/ === "abc"'),
                 "Regexp#=== is false when the pattern does not match"
  end

  def test_source_returns_pattern_text
    assert_equal "a.b", eval_regexp("/a.b/.source"),
                 "Regexp#source returns the original pattern text"
  end

  def test_casefold_true_with_ignorecase
    assert_equal true, eval_regexp("/x/i.casefold?"),
                 "Regexp#casefold? is true for an /i pattern"
  end

  def test_casefold_false_without_ignorecase
    assert_equal false, eval_regexp("/x/.casefold?"),
                 "Regexp#casefold? is false for a pattern without /i"
  end

  def test_inspect_renders_literal_form
    assert_equal "/a.b/i", eval_regexp("/a.b/i.inspect"),
                 "Regexp#inspect renders the /source/flags literal form"
  end

  def test_to_s_renders_inline_flag_group
    assert_equal "(?-mix:a.b)", eval_regexp("/a.b/.to_s"),
                 "Regexp#to_s renders the (?-mix:source) inline-flag form"
  end

  def test_escape_quotes_metacharacters
    assert_equal 'a\.b\*c\+d', eval_regexp('Regexp.escape("a.b*c+d")'),
                 "Regexp.escape backslash-quotes regexp metacharacters"
  end

  def test_compile_is_new_and_matches
    assert_equal "aaa", eval_regexp('Regexp.compile("a+").match("baaa")[0]'),
                 "Regexp.compile compiles a pattern like Regexp.new"
  end

  def test_runtime_new_round_trips_capture
    assert_equal "bbb", eval_regexp('Regexp.new("a(b+)c").match("xabbbcx")[1]'),
                 "Regexp.new compiles a runtime pattern and yields its capture"
  end

  def test_new_with_ignorecase_flag_matches_case_insensitively
    assert_equal "y", eval_regexp('Regexp.new("ab", Regexp::IGNORECASE).match("AB") ? "y" : "n"'),
                 "Regexp.new honours the Regexp::IGNORECASE option"
  end

  # #options reports MRI's option bits (IGNORECASE = 1, MULTILINE = 4),
  # combined, rather than any engine-internal mask.
  def test_options_reports_mri_ignorecase_bit
    assert_equal 1, eval_regexp("/x/i.options"),
                 "Regexp#options reports MRI's IGNORECASE bit (1)"
  end

  def test_options_combines_mri_bits
    assert_equal 5, eval_regexp("/x/im.options"),
                 "Regexp#options combines MRI's IGNORECASE|MULTILINE bits (5)"
  end

  # An unbalanced pattern fails to compile; the guest RegexpError surfaces to
  # the host as SandboxError.
  def test_invalid_pattern_raises_sandbox_error
    assert_raises(Kobako::SandboxError,
                  "an invalid pattern surfaces a guest RegexpError as SandboxError") do
      eval_regexp('Regexp.new("(")')
    end
  end

  # A backreference pattern that blows past the engine's backtracking limit
  # raises rather than running unbounded.
  def test_catastrophic_backtracking_raises_rather_than_hanging
    assert_raises(Kobako::SandboxError,
                  "a fancy pattern past the backtrack limit raises, not hangs") do
      eval_regexp('/(a+)+\1$/.match("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!")')
    end
  end

  # The gem provides RegexpError as a StandardError subclass, so guest code
  # can rescue a bad pattern with a bare rescue or rescue StandardError.
  def test_regexp_error_is_a_standard_error
    assert_equal true, eval_regexp("RegexpError.ancestors.include?(StandardError)"),
                 "RegexpError is a StandardError subclass guest code can rescue"
  end
end
