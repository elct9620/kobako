# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — JSON.parse value mapping and malformed handling through
# the real json guest (docs/json.md JS-01, JS-04, JS-05).
class TestJsonParse < Minitest::Test
  include JsonGuestHelper

  # JS-01: each JSON value maps to its native mruby counterpart, with
  # object keys defaulting to String.
  def test_js01_parse_maps_each_json_value_to_its_native_type
    result = eval_json(<<~RUBY)
      JSON.parse('{"s":"x","n":1,"f":1.5,"t":true,"f2":false,"z":null,"a":[1,2]}')
    RUBY

    assert_equal({ "s" => "x", "n" => 1, "f" => 1.5, "t" => true, "f2" => false, "z" => nil, "a" => [1, 2] },
                 result,
                 "JSON.parse through the json guest must map each JSON value to its native mruby type with String keys")
  end

  # JS-01: parse accepts a bare top-level scalar, not only objects/arrays.
  def test_js01_parse_accepts_a_top_level_scalar
    assert_equal 42, eval_json('JSON.parse("42")'),
                 "JSON.parse of a bare JSON number through the json guest must yield the scalar"
  end

  # JS-04: object member order is preserved, so the resulting Hash iterates
  # in source order rather than sorted or hashed order.
  def test_js04_parse_preserves_object_member_order
    keys = eval_json('JSON.parse(%q({"z":1,"a":2,"m":3})).keys')

    assert_equal %w[z a m], keys,
                 "JSON.parse through the json guest must preserve JSON object member order in the resulting Hash"
  end

  # JS-05: malformed, truncated, and trailing-content input each raise
  # JSON::ParserError, attributed as Kobako::SandboxError when uncaught.
  def test_js05_malformed_input_raises_parser_error
    ['JSON.parse("{bad}")', 'JSON.parse("[1,2")', 'JSON.parse("")', 'JSON.parse("1 2")'].each do |code|
      assert_guest_raises "JSON::ParserError", code
    end
  end

  # JS-05: a guest may rescue JSON::ParserError like any other exception —
  # it is a real guest exception, not a host trap.
  def test_js05_parser_error_is_rescuable_in_guest
    result = eval_json('begin; JSON.parse("{bad}"); "no-error"; rescue JSON::ParserError; "rescued"; end')

    assert_equal "rescued", result,
                 "a guest must be able to rescue JSON::ParserError raised by a malformed JSON.parse"
  end
end
