# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — JSON.parse symbolize_names option through the real json
# guest (docs/json.md JS-02).
class TestJsonParseSymbolize < Minitest::Test
  include JsonGuestHelper

  # JS-02: symbolize_names: true makes every object key a Symbol; values
  # are unaffected.
  def test_js02_symbolize_names_makes_keys_symbols
    result = eval_json('JSON.parse(%q({"a":1,"b":{"c":2}}), symbolize_names: true)')

    assert_equal({ a: 1, b: { c: 2 } }, result,
                 "JSON.parse(symbolize_names: true) through the json guest must key every nested object with Symbols")
  end

  # JS-02: the default (option absent or false) keeps String keys.
  def test_js02_default_keeps_string_keys
    assert_equal({ "a" => 1 }, eval_json('JSON.parse(%q({"a":1}))'),
                 "JSON.parse without symbolize_names through the json guest must keep String keys")
    assert_equal({ "a" => 1 }, eval_json('JSON.parse(%q({"a":1}), symbolize_names: false)'),
                 "JSON.parse(symbolize_names: false) through the json guest must keep String keys")
  end

  # JS-02: only keys change — string values stay String even when keys are
  # symbolized.
  def test_js02_values_are_unaffected_by_symbolize
    result = eval_json('JSON.parse(%q({"k":"v"}), symbolize_names: true)')

    assert_equal({ k: "v" }, result,
                 "JSON.parse(symbolize_names: true) must symbolize keys only, leaving String values intact")
  end
end
