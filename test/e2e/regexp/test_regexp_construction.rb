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
  def test_a_truthy_option_that_is_neither_a_number_nor_text_is_case_insensitive
    assert_equal [1, 1, 1], eval_regexp('[true, 1.5, :m].map { |o| Regexp.new("a", o).options }'),
                 "a truthy non-Integer, non-String option through Regexp.new must make the pattern case-insensitive"
  end

  # @behavior RX-243
  def test_a_false_or_nil_option_leaves_no_option
    assert_equal [0, 0], eval_regexp('[false, nil].map { |o| Regexp.new("a", o).options }'),
                 "false or nil as the option through Regexp.new must leave the pattern without options"
  end

  # @behavior RX-231
  def test_a_flag_string_names_only_the_languages_letters
    assert_equal "ArgumentError", guard_error('Regexp.new("a", "iq")', "ArgumentError"),
                 "a flag string with a letter the language does not name through Regexp.new must raise ArgumentError"
  end

  # @behavior RX-244
  def test_each_letter_a_flag_string_names_becomes_its_own_option
    assert_equal [1, 4, 2, 7, 0], eval_regexp('["i", "m", "x", "imx", ""].map { |f| Regexp.new("a", f).options }'),
                 "each named letter in a flag string through Regexp.new must become its own option"
  end

  # @behavior RX-239
  def test_the_multiline_constant_is_mris_value
    assert_equal 4, eval_regexp("Regexp::MULTILINE"), "Regexp::MULTILINE through #eval must carry MRI's value (4)"
  end
end
