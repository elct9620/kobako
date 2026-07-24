# frozen_string_literal: true

require "test_helper"

# Regexp#to_s rendering contract (docs/regexp.md RX-01). The form is
# (?enabled-disabled:body) with flags in m, i, x order; the -disabled block
# is dropped when all are on, and a whole-source inline-flag group is lifted —
# matching MRI.
class TestRegexpToS < Minitest::Test
  include RegexpGuestHelper

  def test_renders_disabled_block_when_no_flags
    assert_equal "(?-mix:a.b)", eval_regexp("/a.b/.to_s"),
                 "a flag-less pattern through Regexp#to_s must render every flag in the -disabled block"
  end

  def test_renders_single_enabled_flag
    assert_equal "(?i-mx:x)", eval_regexp("/x/i.to_s"),
                 "an /i pattern through Regexp#to_s must show i enabled and m, x disabled"
  end

  def test_omits_disabled_block_when_all_flags_on
    assert_equal "(?mix:abc)",
                 eval_regexp('Regexp.new("abc", Regexp::IGNORECASE | Regexp::EXTENDED | Regexp::MULTILINE).to_s'),
                 "an all-flags-on pattern through Regexp#to_s must omit the -disabled block"
  end

  def test_lifts_whole_source_inline_flag_group
    assert_equal "(?i-mx:abc)", eval_regexp('Regexp.new("(?i:abc)").to_s'),
                 "a whole-source (?i:abc) through Regexp#to_s must lift the inline flag"
  end

  def test_lifts_flagless_group
    assert_equal "(?-mix:abc)", eval_regexp('Regexp.new("(?:abc)").to_s'),
                 "a whole-source (?:abc) through Regexp#to_s must drop the group and keep the body"
  end

  def test_combines_inline_and_outer_flags
    assert_equal "(?mi-x:abc)", eval_regexp('Regexp.new("(?i:abc)", Regexp::MULTILINE).to_s'),
                 "a lifted inline flag through Regexp#to_s must combine with the outer options"
  end

  def test_keeps_partial_span_group_verbatim
    assert_equal "(?-mix:(?im:a)b)", eval_regexp('Regexp.new("(?im:a)b").to_s'),
                 "an inline group not spanning the whole source through Regexp#to_s must stay wrapped verbatim"
  end

  def test_does_not_recurse_into_inner_group
    assert_equal "(?i-mx:(?m:x))", eval_regexp('Regexp.new("(?i:(?m:x))").to_s'),
                 "Regexp#to_s must lift only the outer whole-span group, leaving the inner group as the body"
  end
end
