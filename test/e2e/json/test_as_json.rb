# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the Object#as_json serialization opt-in through the real
# json guest (docs/json.md JS-08).
class TestJsonAsJson < Minitest::Test
  include JsonGuestHelper

  # JS-08: an object opts into generate by overriding as_json to return a
  # JSON-native value; generate encodes that value, so as_json returning the
  # boolean true emits true (not the string "true").
  def test_js08_opt_in_object_serializes_through_its_as_json_value
    assert_equal "true", eval_json("class C; def as_json; true; end; end; JSON.generate(C.new)"),
                 "JSON.generate of an object whose as_json returns true must emit the JSON true"
  end

  # JS-08: the value as_json returns is encoded recursively under the same
  # rules, so a returned Hash becomes a JSON object.
  def test_js08_as_json_value_is_encoded_recursively
    assert_equal '{"a":1}', eval_json("class C; def as_json; { a: 1 }; end; end; JSON.generate(C.new)"),
                 "JSON.generate must encode the structure an object's as_json returns"
  end

  # JS-08: an object that has not overridden as_json hits the raising default
  # and is refused with GeneratorError.
  def test_js08_un_opted_object_raises_generator_error
    assert_guest_raises "JSON::GeneratorError", "JSON.generate(Object.new)"
  end

  # JS-08: generate consults as_json only — overriding to_json does not opt
  # an object in, so it still raises.
  def test_js08_overriding_to_json_does_not_opt_in
    code = 'class C; def to_json; "ignored"; end; end; JSON.generate(C.new)'

    assert_guest_raises "JSON::GeneratorError", code
  end
end
