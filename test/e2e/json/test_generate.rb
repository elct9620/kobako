# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — JSON.generate output through the real json guest
# (docs/json.md JS-06).
class TestJsonGenerate < Minitest::Test
  include JsonGuestHelper

  # JS-06: generate emits compact, well-formed JSON for native values, with
  # nil rendering as null.
  def test_js06_generate_emits_compact_json_for_native_values
    result = eval_json('JSON.generate({"a" => 1, "b" => [true, false, nil], "c" => 1.5})')

    assert_equal '{"a":1,"b":[true,false,null],"c":1.5}', result,
                 "JSON.generate of native values through the json guest must emit compact, well-formed JSON"
  end

  # JS-06: control characters are escaped — a newline and tab render as
  # their two-character JSON escapes. The input is built from char codes so
  # the assertion does not depend on source-level escaping.
  def test_js06_generate_escapes_control_characters
    assert_equal '"\n\t"', eval_json("JSON.generate(10.chr + 9.chr)"),
                 "JSON.generate through the json guest must escape control characters as their JSON escapes"
  end

  # JS-06: escaping is correct enough that any String round-trips — a quote,
  # backslash, and newline survive generate then parse unchanged.
  def test_js06_generate_escaping_round_trips
    result = eval_json("s = 34.chr + 92.chr + 10.chr + 9.chr; JSON.parse(JSON.generate(s)) == s")

    assert_equal true, result,
                 "JSON.generate must escape special characters so JSON.parse reads the original String back"
  end

  # JS-06: a Symbol value renders as its name, and a Symbol key renders as
  # its string form — round-tripping back to String keys on parse.
  def test_js06_symbol_value_and_key_render_as_name
    assert_equal '{"k":"v"}', eval_json("JSON.generate({ k: :v })"),
                 "JSON.generate of Symbol key and value through the json guest must render each as its name"
  end

  # JS-06: a JSON-native scalar key renders as its string form, as in CRuby.
  def test_js06_scalar_key_renders_as_string
    assert_equal '{"1":"a"}', eval_json('JSON.generate({ 1 => "a" })'),
                 "JSON.generate of an Integer key through the json guest must render the key as its string form"
  end

  # JS-06: a NaN or infinite Float raises GeneratorError, as CRuby does
  # without allow_nan.
  def test_js06_nan_and_infinity_raise_generator_error
    assert_guest_raises "JSON::GeneratorError", "JSON.generate(0.0 / 0.0)"
    assert_guest_raises "JSON::GeneratorError", "JSON.generate(1.0 / 0.0)"
  end

  # JS-06: an Array or Hash key is not a usable JSON key and raises rather
  # than stringify.
  def test_js06_non_scalar_key_raises_generator_error
    assert_guest_raises "JSON::GeneratorError", "JSON.generate({ [1] => 2 })"
  end

  # JS-06: a Float key renders as its string form, like the other
  # JSON-native scalar keys.
  def test_js06_float_key_renders_as_string
    assert_equal '{"1.5":2}', eval_json("JSON.generate({ 1.5 => 2 })"),
                 "JSON.generate of a Float key through the json guest must render the key as its string form"
  end

  # JS-06: a String carries JSON text, so a non-UTF-8 byte sequence is
  # refused rather than lossily transcoded.
  def test_js06_non_utf8_string_raises_generator_error
    assert_guest_raises "JSON::GeneratorError", "JSON.generate(255.chr)"
  end

  # JS-06: a value is classified by its native type, not its class identity,
  # so a subclass of a JSON-native type serializes as that native kind.
  def test_js06_native_subclass_serializes_as_native_kind
    assert_equal "[1,2]",
                 eval_json("class A < Array; end; a = A.new; a.push(1); a.push(2); JSON.generate(a)"),
                 "JSON.generate of an Array subclass through the json guest must serialize it as a JSON array"
    assert_equal '{"k":1}',
                 eval_json('class H < Hash; end; h = H.new; h["k"] = 1; JSON.generate(h)'),
                 "JSON.generate of a Hash subclass through the json guest must serialize it as a JSON object"
    assert_equal '"x"',
                 eval_json('class S < String; end; JSON.generate(S.new("x"))'),
                 "JSON.generate of a String subclass through the json guest must serialize it as a JSON string"
  end
end
