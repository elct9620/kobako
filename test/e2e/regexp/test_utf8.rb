# frozen_string_literal: true

require "test_helper"

# UTF-8 matching contract (SPEC.md B-41). Literal multibyte patterns match
# their substrings and offsets are byte-based.
class TestRegexpUtf8 < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-082
  def test_multibyte_literal_slice
    assert_equal "漢字", eval_regexp('"漢字abc"[/漢字/]'),
                 "a multibyte literal pattern slices the matching substring"
  end

  # @behavior RX-083
  def test_multibyte_match_reports_byte_offset
    assert_equal 4, eval_regexp('"x漢字" =~ /字/'),
                 "=~ on a multibyte string reports the byte offset, not the char index"
  end

  # @behavior RX-084
  def test_multibyte_capture_round_trips_as_array
    assert_equal %w[漢字 漢字], eval_regexp('"abc漢字def".match(/(漢字)/).to_a'),
                 "a multibyte capture group must round-trip through the host wire as an Array of substrings"
  end

  # docs/regexp.md RX-01: the shorthand classes are ASCII, but a negated
  # shorthand inside a character class keeps the engine's Unicode category
  # semantics. The fullwidth digit ５ (Unicode Nd, not ASCII 0-9) tells the
  # two apart.
  # @behavior RX-085
  def test_negated_shorthand_is_ascii_outside_a_class
    assert_equal 0, eval_regexp('"５" =~ /\D/'),
                 "a non-ASCII digit through /\\D/ must match (ASCII negation)"
  end

  # @behavior RX-086
  def test_negated_shorthand_inside_a_class_is_unicode
    assert_nil eval_regexp('"５" =~ /[\D]/'),
               "a non-ASCII digit through /[\\D]/ must not match (Unicode category)"
  end
end
