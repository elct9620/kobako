# frozen_string_literal: true

require "test_helper"

# E2E — what JSON.generate does with a Hash key and with an object that
# only looks serializable, through the real json guest. A key becomes text
# only when it has a string form the language already gives it; anything
# else, opted in or not, is refused rather than guessed at.
class TestJsonGenerateKeys < Minitest::Test
  include JsonGuestHelper

  # @behavior JS-051
  def test_a_nil_key_is_written_as_its_string_form
    assert_equal '{"":1}', eval_json("JSON.generate({ nil => 1 })"),
                 "JSON.generate of a Hash keyed by nil must write the key as its string form"
  end

  # @behavior JS-052
  def test_a_boolean_key_is_written_as_its_string_form
    assert_equal '{"true":1,"false":2}', eval_json("JSON.generate({ true => 1, false => 2 })"),
                 "JSON.generate of a Hash keyed by booleans must write each key as its string form"
  end

  # @behavior JS-053
  def test_a_plain_object_key_is_refused
    assert_guest_raises "JSON::GeneratorError", "JSON.generate({ Object.new => 1 })"
  end

  # @behavior JS-054
  def test_an_opted_in_object_is_still_refused_as_a_key
    assert_guest_raises "JSON::GeneratorError",
                        "class C; def as_json; 1; end; end; JSON.generate({ C.new => 1 })"
  end

  # Each answers a question a serializer might read as consent; only the
  # hook is consent.
  LOOKALIKES = {
    "respond_to?" => "def respond_to?(*) = true",
    "to_a" => "def to_a = [1]",
    "to_h" => "def to_h = { a: 1 }"
  }.freeze

  # @behavior JS-055
  def test_an_object_that_only_looks_serializable_is_refused
    LOOKALIKES.each_value do |definition|
      assert_guest_raises "JSON::GeneratorError", "class C; #{definition}; end; JSON.generate(C.new)"
    end
  end

  # @behavior JS-056
  def test_what_the_hook_answers_is_held_to_the_depth_bound
    assert_guest_raises "JSON::GeneratorError",
                        "class C; def as_json; a = []; 127.times { a = [a] }; a; end; end; JSON.generate(C.new)"
  end

  ERROR_ROOTS = <<~RUBY
    [JSON::ParserError.ancestors.include?(JSON::JSONError),
     JSON::GeneratorError.ancestors.include?(JSON::JSONError),
     JSON::JSONError.ancestors.include?(StandardError)]
  RUBY

  # @behavior JS-057
  def test_the_error_classes_share_one_standard_error_root
    assert_equal [true, true, true], eval_json(ERROR_ROOTS),
                 "JSON::ParserError and JSON::GeneratorError must both descend from JSON::JSONError, " \
                 "itself a StandardError"
  end
end
