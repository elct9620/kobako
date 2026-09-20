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

  # The option is read by the name it is written under, so a String spelling
  # of it is a keyword this parse does not name and stays in the rest.
  # @behavior JS-016
  def test_string_keyed_option_does_not_symbolize
    assert_equal({ "a" => 1 }, eval_json('JSON.parse(%q({"a":1}), **{"symbolize_names" => true})'),
                 "JSON.parse given the symbolize_names option under a String spelling must " \
                 "leave it unread, keeping String keys")
  end

  # The surface takes a document and keywords, so a second positional argument
  # is a call shape it never had; reading options out of one accepted calls MRI
  # refuses and swallowed whatever was passed when it was not a Hash.
  # @behavior JS-046
  def test_a_second_positional_argument_is_refused
    err = assert_guest_raises("ArgumentError", 'JSON.parse(%q({"a":1}), {symbolize_names: true})')

    assert_match(/wrong number of arguments/, err.message,
                 "JSON.parse given its options as a positional Hash must be refused for its " \
                 "argument count rather than reading them")
  end

  # @behavior JS-017
  def test_values_are_unaffected_by_symbolize
    result = eval_json('JSON.parse(%q({"k":"v"}), symbolize_names: true)')

    assert_equal({ k: "v" }, result,
                 "JSON.parse(symbolize_names: true) must symbolize keys only, leaving String values intact")
  end
end
