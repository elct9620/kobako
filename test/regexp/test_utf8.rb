# frozen_string_literal: true

require "test_helper"

# UTF-8 matching contract (SPEC.md B-41). Literal multibyte patterns match
# their substrings and offsets are byte-based.
class TestRegexpUtf8 < Minitest::Test
  include RegexpGuestHelper

  def test_multibyte_literal_slice
    assert_equal "漢字", eval_regexp('"漢字abc"[/漢字/]'),
                 "a multibyte literal pattern slices the matching substring"
  end

  def test_multibyte_match_reports_byte_offset
    assert_equal 4, eval_regexp('"x漢字" =~ /字/'),
                 "=~ on a multibyte string reports the byte offset, not the char index"
  end
end
