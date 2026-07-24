# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the shared nesting bound through the real json guest
# (docs/json.md JS-09). parse and generate reject at the same depth, so a
# generated structure always re-parses.
class TestJsonNestingDepth < Minitest::Test
  include JsonGuestHelper

  # JS-09: parse accepts a structure at the bound and rejects one nested a
  # level past it.
  def test_js09_parse_accepts_the_bound_and_rejects_beyond
    assert_equal "Array", eval_json('JSON.parse("[" * 127 + "]" * 127).class.to_s'),
                 "JSON.parse of 127 nested arrays through the json guest must succeed at the depth bound"
    assert_guest_raises "JSON::ParserError", 'JSON.parse("[" * 128 + "]" * 128)'
  end

  # JS-09: generate rejects at the same depth parse does, so the two paths
  # agree. 127 nested levels generate; the 128th raises.
  def test_js09_generate_rejects_at_the_same_depth_as_parse
    ok = eval_json("a = []; 126.times { a = [a] }; JSON.generate(a).start_with?(\"[\")")
    assert_equal true, ok,
                 "JSON.generate of 127 nested arrays through the json guest must succeed at the depth bound"
    assert_guest_raises "JSON::GeneratorError", "a = []; 127.times { a = [a] }; JSON.generate(a)"
  end
end
