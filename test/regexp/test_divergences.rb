# frozen_string_literal: true

require "test_helper"

# Deliberate divergences from the C gem (SPEC.md B-41). The C gem returns
# an Onigmo internal value or a buggy result for these; the Rust gem
# follows MRI instead, so they are asserted against the Rust guest only.
# Invalid-pattern handling is shared and asserted on both guests.
class TestRegexpDivergences < Minitest::Test
  include RegexpParityHelper

  # The C gem returns Onigmo's option bitmask (57345 for /i); the Rust gem
  # exposes MRI's IGNORECASE = 1.
  def test_options_reports_mri_ignorecase_bit
    assert_rust_only(1, "/x/i.options",
                     "Regexp#options reports MRI's IGNORECASE bit (1), not Onigmo's mask")
  end

  # MRI combines option bits; /im is IGNORECASE | MULTILINE = 1 | 4.
  def test_options_combines_mri_bits
    assert_rust_only(5, "/x/im.options",
                     "Regexp#options combines MRI's IGNORECASE|MULTILINE bits (5)")
  end

  # The C gem leaves $1 pinned to the first match across a gsub block; MRI
  # refreshes it on each iteration.
  def test_dollar1_refreshes_per_gsub_iteration
    assert_rust_only("a1!b2!", '"a1b2".gsub(/(\d)/){ $1 + "!" }',
                     "$1 inside a gsub block refreshes to each iteration's capture")
  end

  # An unbalanced pattern fails to compile and surfaces as SandboxError on
  # both guests.
  def test_invalid_pattern_raises_sandbox_error
    assert_parity_raises(Kobako::SandboxError, 'Regexp.new("(")',
                         "an invalid pattern surfaces a guest RegexpError as SandboxError")
  end

  # A fancy pattern (backreference) that blows past the engine's backtracking
  # limit raises rather than running unbounded. The C gem's Onigmo engine
  # treats this case differently, so it is asserted on the Rust gem only.
  def test_catastrophic_backtracking_raises_rather_than_hanging
    assert_rust_raises(Kobako::SandboxError,
                       '/(a+)+\1$/.match("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!")',
                       "a fancy pattern past the backtrack limit raises, not hangs")
  end
end
