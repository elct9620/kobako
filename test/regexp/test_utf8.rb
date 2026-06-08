# frozen_string_literal: true

require "test_helper"

# UTF-8 matching parity (SPEC.md B-41). Literal multibyte patterns match
# the same substrings on both guests, and offsets are byte-based. Expected
# values are the C-gem (Onigmo) oracle harvested from data/kobako.wasm.
class TestRegexpUtf8 < Minitest::Test
  include RegexpParityHelper

  def test_multibyte_literal_slice
    assert_parity("漢字", '"漢字abc"[/漢字/]',
                  "a multibyte literal pattern slices the matching substring")
  end

  def test_multibyte_match_reports_byte_offset
    assert_parity(4, '"x漢字" =~ /字/',
                  "=~ on a multibyte string reports the byte offset, not the char index")
  end
end
