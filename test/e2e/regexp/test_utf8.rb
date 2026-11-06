# frozen_string_literal: true

require "test_helper"

# UTF-8 matching contract. Literal multibyte patterns match
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

  # The fullwidth digit ５ is a Unicode digit but no ASCII one, so it tells
  # an ASCII shorthand from a Unicode one.
  # @behavior RX-085
  def test_negated_shorthand_is_ascii_outside_a_class
    assert_equal 0, eval_regexp('"５" =~ /\D/'),
                 "a non-ASCII digit through /\\D/ must match (ASCII negation)"
  end

  # @behavior RX-086
  def test_negated_shorthand_inside_a_class_is_ascii
    assert_equal [0, 0], eval_regexp('["５" =~ /[\D]/, "é" =~ /[a\W]/]'),
                 "a non-ASCII digit through /[\\D]/ and a letter through /[a\\W]/ must match (ASCII negation)"
  end

  # @behavior RX-293
  def test_a_posix_bracket_reads_unicode
    assert_equal [0, nil], eval_regexp('["é" =~ /[[:alpha:]]/, "é" =~ /[[:^alpha:]]/]'),
                 "a non-ASCII letter through /[[:alpha:]]/ must match and through /[[:^alpha:]]/ must not"
  end

  # @behavior RX-294
  def test_the_unicode_option_widens_the_shorthand_classes
    assert_equal [0, 0, nil], eval_regexp('["é" =~ /(?u)\w/, "５" =~ /(?u:\d)/, "é" =~ /\w/]'),
                 "a non-ASCII letter and digit through /(?u)\\w/ and /(?u:\\d)/ must match where /\\w/ does not"
  end

  # @behavior RX-295
  def test_the_ascii_option_narrows_a_posix_bracket
    assert_equal [nil, 0], eval_regexp('["é" =~ /(?a)[[:alpha:]]/, "é" =~ /(?a:[[:^alpha:]])/]'),
                 "a non-ASCII letter through /(?a)[[:alpha:]]/ must not match"
  end

  # @behavior RX-296
  def test_the_ascii_option_reads_a_word_boundary_by_ascii
    assert_equal [2, nil], eval_regexp('["éb" =~ /(?a)\bb/, "éb" =~ /\bb/]'),
                 "a boundary after a non-ASCII letter through /(?a)\\bb/ must match where /\\bb/ does not"
  end

  # U+200B is a format character, which CRuby's Graph and Print take in.
  # @behavior RX-299
  def test_a_property_reads_the_set_cruby_gives_its_name
    assert_equal [0, 0, 0, 0],
                 eval_regexp('["​" =~ /\p{Graph}/, "​" =~ /[\p{Print}]/, ' \
                             '"$" =~ /\p{XPosixPunct}/, "" =~ /\p{Unknown}/]'),
                 "a format character through /\\p{Graph}/ and /[\\p{Print}]/, a dollar sign through " \
                 "/\\p{XPosixPunct}/ and a private use character through /\\p{Unknown}/ must match"
  end
end
