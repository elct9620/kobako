# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — JSON.parse symbolize_names option through the real json
# guest.
class TestJsonParseSymbolize < Minitest::Test
  include JsonGuestHelper

  # @behavior JS-013
  def test_symbolize_names_makes_keys_symbols
    result = eval_json('JSON.parse(%q({"a":1,"b":{"c":2}}), symbolize_names: true)')

    assert_equal({ a: 1, b: { c: 2 } }, result,
                 "JSON.parse(symbolize_names: true) through the json guest must key every nested object with Symbols")
  end

  # @behavior JS-014 JS-015
  def test_default_keeps_string_keys
    assert_equal({ "a" => 1 }, eval_json('JSON.parse(%q({"a":1}))'),
                 "JSON.parse without symbolize_names through the json guest must keep String keys")
    assert_equal({ "a" => 1 }, eval_json('JSON.parse(%q({"a":1}), symbolize_names: false)'),
                 "JSON.parse(symbolize_names: false) through the json guest must keep String keys")
  end

  # Only the symbolize_names: keyword is honored, not a String-keyed options
  # Hash.
  # @behavior JS-016
  def test_string_keyed_option_does_not_symbolize
    assert_equal({ "a" => 1 }, eval_json('JSON.parse(%q({"a":1}), {"symbolize_names" => true})'),
                 "JSON.parse with a String-keyed symbolize_names option must be ignored, keeping String keys")
  end

  # @behavior JS-017
  def test_values_are_unaffected_by_symbolize
    result = eval_json('JSON.parse(%q({"k":"v"}), symbolize_names: true)')

    assert_equal({ k: "v" }, result,
                 "JSON.parse(symbolize_names: true) must symbolize keys only, leaving String values intact")
  end
end
