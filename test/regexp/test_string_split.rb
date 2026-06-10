# frozen_string_literal: true

require "test_helper"

# String#split edge cases (docs/regexp.md RX-05). A non-participating capture
# group is omitted (unlike scan, which keeps it as nil), and a zero-width match
# at the start does not emit a leading empty field, as in MRI.
class TestStringSplit < Minitest::Test
  include RegexpGuestHelper

  def test_omits_non_participating_group
    assert_equal %w[a x b], eval_regexp('"axb".split(/(x)(y)?/)'),
                 "String#split must omit a non-participating capture group, not emit nil"
  end

  def test_scan_keeps_non_participating_group_as_nil
    assert_equal [["x", nil]], eval_regexp('"axb".scan(/(x)(y)?/)'),
                 "String#scan must keep a non-participating group as nil (the split/scan contrast)"
  end

  def test_empty_pattern_splits_into_chars_without_leading_blank
    assert_equal %w[a b c], eval_regexp('"abc".split(//)'),
                 "a zero-width pattern through String#split must not emit a leading empty field"
  end

  def test_empty_pattern_respects_limit
    assert_equal %w[a bc], eval_regexp('"abc".split(//, 2)'),
                 "a zero-width split must count real splits toward a positive limit"
  end

  def test_keeps_legitimate_empty_field
    assert_equal ["a", "", "b"], eval_regexp('"a,,b".split(/,/)'),
                 "an empty field between two non-zero-width matches must be kept"
  end
end
