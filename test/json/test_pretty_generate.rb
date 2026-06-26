# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — JSON.pretty_generate layout through the real json guest
# (docs/json.md JS-07).
class TestJsonPrettyGenerate < Minitest::Test
  include JsonGuestHelper

  # The indented layout for a structure that exercises two-space nesting, a
  # space after each colon, one element per line, and inline empty
  # containers.
  EXPECTED = <<~JSON.chomp
    {
      "a": [
        1,
        2
      ],
      "e": [],
      "o": {}
    }
  JSON

  # JS-07: pretty_generate emits the indented layout, leaving an empty array
  # or object inline.
  def test_js07_pretty_generate_indents_with_empty_containers_inline
    assert_equal EXPECTED, eval_json('JSON.pretty_generate({"a" => [1, 2], "e" => [], "o" => {}})'),
                 "JSON.pretty_generate through the json guest must emit the indented layout, empty containers inline"
  end

  # JS-07: the indented output carries the same value as generate — parsing
  # it yields the same tree.
  def test_js07_pretty_output_reparses_to_the_same_tree
    result = eval_json('v = {"a" => [1, 2], "b" => nil}; JSON.parse(JSON.pretty_generate(v)) == v')

    assert_equal true, result,
                 "JSON.pretty_generate output through the json guest must parse back to the same tree as its input"
  end
end
