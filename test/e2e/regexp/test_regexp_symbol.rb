# frozen_string_literal: true

require "test_helper"

# The pattern-taking methods a Symbol has in CRuby, each the same call on the
# Symbol's name as a String. Each pair sends one call to the Symbol and to its
# name, so any answer the two disagree on fails the comparison.
class TestRegexpSymbol < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-253
  def test_symbol_matches_tests_and_slices_by_a_pattern_as_its_name
    calls = [".match(/b(c)/).to_a", ".match(/b/) { |m| m.pre_match }", ".match?(/b/, 2)",
             "[/b(c)/, 1]", ".slice(/c/)"]

    assert_equal [true] * calls.size, eval_regexp(same_as_name(calls)),
                 "Symbol#match, #match?, #[] and #slice by a pattern through Sandbox#eval must answer what " \
                 "the Symbol's name answers as a String"
  end

  # @behavior RX-254
  def test_symbol_slices_by_an_index_as_its_name
    calls = ["[0]", "[1, 2]", ".slice(-1)"]

    assert_equal [true] * calls.size, eval_regexp(same_as_name(calls)),
                 "Symbol#[] and #slice by an index through Sandbox#eval must answer what the Symbol's name " \
                 "answers as a String"
  end

  private

  def same_as_name(calls)
    pairs = calls.map { |call| "[:abc#{call}, 'abc'#{call}]" }.join(", ")
    "[#{pairs}].map { |sym, str| sym == str }"
  end
end
