# frozen_string_literal: true

require "test_helper"

# In-place String mutation through a Regexp (SPEC.md B-41). The C Onigmo gem
# made String#[]= and String#slice! regexp-aware while delegating a
# non-Regexp argument to the core method. Offsets are byte-based.
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
end
