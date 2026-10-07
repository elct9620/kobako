# frozen_string_literal: true

require "test_helper"

# E2E — the Object#as_json serialization opt-in through the real
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

  # A hook that edits the Hash being written — the one guest code that runs
  # mid-walk. CRuby's generator skips a removed key and refuses an added one.
  MUTATING_HOOK = <<~RUBY
    class Editor
      def initialize(hash, &edit) = (@hash = hash; @edit = edit)
      def as_json = (@edit.call(@hash); "x")
    end
  RUBY

  # @behavior JS-060
  def test_key_removed_by_a_hook_is_not_written
    code = "#{MUTATING_HOOK}h = {}; h[:a] = Editor.new(h) { |x| x.delete(:b) }; h[:b] = 2; JSON.generate(h)"

    assert_equal '{"a":"x"}', eval_json(code),
                 "JSON.generate of a Hash whose hook removes an unwritten key must leave that key out"
  end

  # One added key, so the table keeps its capacity and only the walk itself
  # can notice the change.
  # @behavior JS-061
  def test_key_added_by_a_hook_is_refused
    code = "#{MUTATING_HOOK}h = {}; h[:a] = Editor.new(h) { |x| x[:c] = 3 }; h[:b] = 2; JSON.generate(h)"

    err = assert_guest_raises "RuntimeError", code
    assert_includes err.message, "can't add a new key into hash during iteration",
                    "JSON.generate of a Hash whose hook adds a key must raise CRuby's iteration error"
  end
end
