# frozen_string_literal: true

require "test_helper"

# In-place String mutation through a Regexp (SPEC.md B-41). String#[]= and
# String#slice! are regexp-aware while delegating a non-Regexp argument to
# the core method. Offsets are byte-based.
class TestRegexpStringMutation < Minitest::Test
  include RegexpGuestHelper

  def test_aset_replaces_whole_match_in_place
    assert_equal "heLLo", eval_regexp('s = "hello"; s[/l+/] = "LL"; s'),
                 "String#[]= with a Regexp overwrites the whole matched region"
  end

  def test_aset_replaces_group_in_place
    assert_equal "a1Xb", eval_regexp('s = "a12b"; s[/(\d)(\d)/, 2] = "X"; s'),
                 "String#[]= with a Regexp and group index overwrites that group"
  end

  def test_aset_with_string_key_delegates_to_core
    assert_equal "hELLo", eval_regexp('s = "hello"; s["ell"] = "ELL"; s'),
                 "String#[]= with a String argument delegates to the core method"
  end

  def test_aset_with_integer_args_delegates_to_core
    assert_equal "HEllo", eval_regexp('s = "hello"; s[0, 2] = "HE"; s'),
                 "String#[]= with Integer arguments delegates to the core method"
  end

  def test_aset_raises_when_regexp_does_not_match
    assert_raises(Kobako::SandboxError,
                  "String#[]= on a non-matching Regexp surfaces an error") do
      eval_regexp('s = "abc"; s[/\d/] = "x"')
    end
  end

  def test_slice_bang_removes_regexp_match
    assert_equal %w[ll heo], eval_regexp('s = "hello"; r = s.slice!(/l+/); [r, s]'),
                 "String#slice! with a Regexp returns and removes the matched substring"
  end

  def test_slice_bang_returns_nil_when_regexp_does_not_match
    assert_equal [nil, "abc"], eval_regexp('s = "abc"; r = s.slice!(/\d/); [r, s]'),
                 "String#slice! returns nil and leaves the string when the Regexp misses"
  end

  def test_slice_bang_restores_last_match_to_its_own_match
    assert_equal "ll", eval_regexp('s = "hello"; s.slice!(/l+/); $~[0]'),
                 "String#slice! restores $~ to its own match after the inner delete"
  end

  def test_slice_bang_with_integer_delegates_to_core
    assert_equal %w[h ello], eval_regexp('s = "hello"; r = s.slice!(0); [r, s]'),
                 "String#slice! with an Integer removes that character via the core path"
  end

  def test_slice_bang_with_integer_length_delegates_to_core
    assert_equal %w[el hlo], eval_regexp('s = "hello"; r = s.slice!(1, 2); [r, s]'),
                 "String#slice! with Integer start and length removes that range"
  end

  def test_slice_bang_with_string_delegates_to_core
    assert_equal %w[ll heo], eval_regexp('s = "hello"; r = s.slice!("ll"); [r, s]'),
                 "String#slice! with a String removes its first occurrence"
  end
end
