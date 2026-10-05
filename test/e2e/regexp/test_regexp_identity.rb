# frozen_string_literal: true

require "test_helper"

# What a Regexp is, as distinct from what it matches: the option bits it
# reports, when two patterns are the same pattern, and the quoting alias
# that answers as escaping does.
class TestRegexpIdentity < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-197
  def test_the_extended_flag_reports_bit_two
    assert_equal 2, eval_regexp("/a/x.options"),
                 "a pattern carrying the extended flag through Regexp#options must report 2 (MRI)"
  end

  # @behavior RX-198
  def test_patterns_are_equal_when_source_and_options_are
    assert_equal [true, false, false], eval_regexp("[/a/i == /a/i, /a/i == /b/i, /a/i == /a/]"),
                 "two patterns through Regexp#== must be equal exactly when their source and " \
                 "options are both equal"
  end

  # @behavior RX-199
  def test_quote_escapes_as_escape_does
    assert_equal true, eval_regexp('Regexp.quote("a.b*c?") == Regexp.escape("a.b*c?")'),
                 "Regexp.quote must escape exactly as Regexp.escape does"
  end
end
