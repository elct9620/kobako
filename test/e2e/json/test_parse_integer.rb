# frozen_string_literal: true

require "test_helper"

# E2E — JSON.parse integer policy through the real json guest. The guest
# Integer has no width of its own, so every integer literal reads as that
# exact Integer, as in CRuby, rather than degrade to a Float.
class TestJsonParseInteger < Minitest::Test
  include JsonGuestHelper

  # @behavior JS-006 JS-007
  def test_in_range_integer_stays_integer
    assert_equal 9_223_372_036_854_775_807, eval_json('JSON.parse("9223372036854775807")'),
                 "JSON.parse of the largest 64-bit value through the json guest must yield that Integer"
    assert_equal(-100, eval_json('JSON.parse("-100")'),
                 "JSON.parse of a small negative integer through the json guest must yield that Integer")
  end

  # An id past 2**53 would lose its low digits as a Float, so it stays an
  # Integer as long as the guest's width holds it.
  # @behavior JS-009
  def test_integer_past_exact_float_range_stays_exact
    assert_equal 9_007_199_254_740_993, eval_json('JSON.parse("9007199254740993")'),
                 "JSON.parse of 2**53 + 1 through the json guest must yield that exact Integer"
  end

  # One literal per way the guest builds the Integer: past 64 bits on each
  # sign, past 128 bits on each sign. The guest answers each as its digits
  # so the check reads what the guest holds, not what the wire could carry.
  # @behavior JS-010
  def test_integer_past_64_bits_stays_exact
    literals = %w[
      9223372036854775808 -9223372036854775809
      340282366920938463463374607431768211456 -340282366920938463463374607431768211457
    ]
    source = "#{literals.inspect}.map { |s| JSON.parse(s).to_s }"

    assert_equal literals, eval_json(source),
                 "JSON.parse of integers past 64 and 128 bits through the json guest must yield " \
                 "each exact Integer"
  end

  # A JSON real maps to Float regardless of magnitude.
  # @behavior JS-011 JS-012
  def test_real_maps_to_float
    assert_equal 1.5, eval_json('JSON.parse("1.5")'),
                 "JSON.parse of a JSON real through the json guest must yield a Float"
    assert_equal "Float", eval_json('JSON.parse("1e2").class.to_s'),
                 "JSON.parse of an exponent-form real through the json guest must yield a Float"
  end
end
