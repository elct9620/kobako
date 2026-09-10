# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — JSON.parse value mapping and malformed handling through
# the real json guest.
class TestJsonParse < Minitest::Test
  include JsonGuestHelper

  # @behavior JS-001
  def test_parse_maps_each_json_value_to_its_native_type
    result = eval_json(<<~RUBY)
      JSON.parse('{"s":"x","n":1,"f":1.5,"t":true,"f2":false,"z":null,"a":[1,2]}')
    RUBY

    assert_equal({ "s" => "x", "n" => 1, "f" => 1.5, "t" => true, "f2" => false, "z" => nil, "a" => [1, 2] },
                 result,
                 "JSON.parse through the json guest must map each JSON value to its native mruby type with String keys")
  end

  # @behavior JS-002
  def test_parse_accepts_a_top_level_scalar
    assert_equal 42, eval_json('JSON.parse("42")'),
                 "JSON.parse of a bare JSON number through the json guest must yield the scalar"
  end

  # @behavior JS-003
  def test_parse_preserves_object_member_order
    keys = eval_json('JSON.parse(%q({"z":1,"a":2,"m":3})).keys')

    assert_equal %w[z a m], keys,
                 "JSON.parse through the json guest must preserve JSON object member order in the resulting Hash"
  end

  # Malformed, truncated, and trailing-content input each raise
  # JSON::ParserError, attributed as Kobako::SandboxError when uncaught.
  # @behavior JS-004
  def test_malformed_input_raises_parser_error
    ['JSON.parse("{bad}")', 'JSON.parse("[1,2")', 'JSON.parse("")', 'JSON.parse("1 2")'].each do |code|
      assert_guest_raises "JSON::ParserError", code
    end
  end

  # JSON::ParserError is a real guest exception, not a host trap.
  # @behavior JS-005
  def test_parser_error_is_rescuable_in_guest
    result = eval_json('begin; JSON.parse("{bad}"); "no-error"; rescue JSON::ParserError; "rescued"; end')

    assert_equal "rescued", result,
                 "a guest must be able to rescue JSON::ParserError raised by a malformed JSON.parse"
  end
end
