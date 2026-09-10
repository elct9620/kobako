# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the Object#as_json serialization opt-in through the real
# json guest.
class TestJsonAsJson < Minitest::Test
  include JsonGuestHelper

  # generate encodes the JSON-native value as_json returns, so as_json
  # returning the boolean true emits true (not the string "true").
  # @behavior JS-033
  def test_opt_in_object_serializes_through_its_as_json_value
    assert_equal "true", eval_json("class C; def as_json; true; end; end; JSON.generate(C.new)"),
                 "JSON.generate of an object whose as_json returns true must emit the JSON true"
  end

  # @behavior JS-034
  def test_as_json_value_is_encoded_recursively
    assert_equal '{"a":1}', eval_json("class C; def as_json; { a: 1 }; end; end; JSON.generate(C.new)"),
                 "JSON.generate must encode the structure an object's as_json returns"
  end

  # An object that has not overridden as_json hits the raising default.
  # @behavior JS-035
  def test_un_opted_object_raises_generator_error
    assert_guest_raises "JSON::GeneratorError", "JSON.generate(Object.new)"
  end

  # generate consults as_json only — overriding to_json does not opt an
  # object in, so it still raises.
  # @behavior JS-036
  def test_overriding_to_json_does_not_opt_in
    code = 'class C; def to_json; "ignored"; end; end; JSON.generate(C.new)'

    assert_guest_raises "JSON::GeneratorError", code
  end
end
