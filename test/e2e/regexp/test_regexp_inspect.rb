# frozen_string_literal: true

require "test_helper"

# Regexp#inspect rendering contract (docs/regexp.md RX-01). The source is
# rendered as a regexp literal: / is escaped to \/, a non-whitespace control
# character becomes \xHH, and printable characters, multibyte UTF-8, and the
# whitespace controls pass through literally — matching MRI.
class TestRegexpInspect < Minitest::Test
  include RegexpGuestHelper

  def test_renders_source_and_flags
    assert_equal "/a.b/i", eval_regexp("/a.b/i.inspect"),
                 "a literal /a.b/i through Regexp#inspect must render as /source/flags"
  end

  def test_escapes_slash_in_source
    assert_equal "/a\\/b/", eval_regexp('Regexp.new("a/b").inspect'),
                 "a source containing / through Regexp#inspect must escape it to \\/"
  end

  def test_keeps_whitespace_controls_literal
    assert_equal "/a\nb/", eval_regexp('Regexp.new("a\nb").inspect'),
                 "a newline in the source through Regexp#inspect must stay literal (MRI)"
  end

  def test_escapes_other_control_characters_as_hex
    assert_equal "/a\\x1Bb/", eval_regexp('Regexp.new("a\x1bb").inspect'),
                 "an ESC control byte in the source through Regexp#inspect must render as uppercase-hex \\x1B"
  end

  def test_preserves_multibyte_utf8
    assert_equal "/café/", eval_regexp('Regexp.new("café").inspect'),
                 "multibyte UTF-8 in the source through Regexp#inspect must pass through unescaped"
  end
end
