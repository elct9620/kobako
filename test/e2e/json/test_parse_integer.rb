# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — JSON.parse integer-range policy through the real json
# guest (docs/json.md JS-03). The guest Integer is 32-bit, so the policy
# keeps an in-range integer as Integer, widens an exactly-representable
# magnitude to Float, and refuses anything beyond exact Float range rather
# than silently degrade it.
class TestJsonParseInteger < Minitest::Test
  include JsonGuestHelper

  # JS-03: a value within the guest's 32-bit Integer width stays Integer.
  def test_js03_in_range_integer_stays_integer
    assert_equal "Integer", eval_json('JSON.parse("2147483647").class.to_s'),
                 "JSON.parse of the largest 32-bit value through the json guest must yield an Integer"
    assert_equal(-100, eval_json('JSON.parse("-100")'),
                 "JSON.parse of a small negative integer through the json guest must yield that Integer")
  end

  # JS-03: a magnitude past the 32-bit width but exact in an f64 widens to
  # Float — covering millisecond timestamps and ordinary 53-bit ids.
  def test_js03_exact_float_range_widens_to_float
    assert_equal "Float", eval_json('JSON.parse("2147483648").class.to_s'),
                 "JSON.parse of 2**31 through the json guest must widen to Float (beyond the 32-bit Integer width)"
    assert_equal "Float", eval_json('JSON.parse("9007199254740992").class.to_s'),
                 "JSON.parse of 2**53 through the json guest must yield an exact Float"
  end

  # JS-03: a magnitude beyond exact Float range raises rather than lose
  # precision — including an integer larger than any 64-bit width.
  def test_js03_inexact_magnitude_raises_parser_error
    assert_guest_raises "JSON::ParserError", 'JSON.parse("9007199254740993")'
    assert_guest_raises "JSON::ParserError", 'JSON.parse("100000000000000000000")'
  end

  # JS-03: a JSON real maps to Float regardless of magnitude.
  def test_js03_real_maps_to_float
    assert_equal 1.5, eval_json('JSON.parse("1.5")'),
                 "JSON.parse of a JSON real through the json guest must yield a Float"
    assert_equal "Float", eval_json('JSON.parse("1e2").class.to_s'),
                 "JSON.parse of an exponent-form real through the json guest must yield a Float"
  end
end
