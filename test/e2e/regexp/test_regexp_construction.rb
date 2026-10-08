# frozen_string_literal: true

require "test_helper"

# What a pattern can be built from. Rendering any other value as text
# would compile it, and nil would become the empty pattern that matches
# everywhere, so each is refused instead.
class TestRegexpConstruction < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-227
  def test_a_source_that_is_neither_a_string_nor_a_pattern_is_refused
    %w[1 :a nil].each do |source|
      %w[new compile].each do |name|
        assert_equal "TypeError", guard_error("Regexp.#{name}(#{source})", "TypeError"),
                     "#{source} as the source through Regexp.#{name} must raise TypeError"
      end
    end
  end

  # @behavior RX-228
  def test_a_pattern_built_from_another_keeps_its_source_and_options
    assert_equal ["a.", 1, true],
                 eval_regexp("r = Regexp.new(/a./i, Regexp::MULTILINE); [r.source, r.options, r == /a./i]"),
                 "a pattern through Regexp.new must keep its source and options, ignoring the options given alongside"
  end

  # The Symbol is :m, whose name a reader of letters would take for the
  # multiline flag rather than for a true value.
  # @behavior RX-230
  def test_an_option_that_is_neither_a_number_nor_text_is_read_by_its_truth
    assert_equal [1, 1, 1, 0, 0], eval_regexp('[true, 1.5, :m, false, nil].map { |o| Regexp.new("a", o).options }'),
                 "a non-Integer, non-String option through Regexp.new must make the pattern case-insensitive " \
                 "when true and leave it without options when false or nil"
  end
end
