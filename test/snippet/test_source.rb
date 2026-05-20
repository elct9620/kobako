# frozen_string_literal: true

require "test_helper"

# Unit tests for Kobako::Snippet::Source — the value object representing
# a single #preload(code:, name:) entry held by Kobako::Snippet::Table
# (docs/behavior.md B-32). Coverage focuses on the Data.define value-
# object semantics; name normalisation and duplicate rejection are the
# Table's responsibility and live in test/snippet/test_table.rb.
class TestSnippetSource < Minitest::Test
  def test_constructs_with_keyword_arguments
    source = Kobako::Snippet::Source.new(name: :Helper, body: "X = 1")

    assert_equal :Helper, source.name
    assert_equal "X = 1", source.body
  end

  def test_value_equality_holds_for_matching_fields
    a = Kobako::Snippet::Source.new(name: :Helper, body: "X = 1")
    b = Kobako::Snippet::Source.new(name: :Helper, body: "X = 1")

    assert_equal a, b
    assert_equal a.hash, b.hash
  end

  def test_distinct_when_name_differs
    a = Kobako::Snippet::Source.new(name: :Helper, body: "X = 1")
    b = Kobako::Snippet::Source.new(name: :Worker, body: "X = 1")

    refute_equal a, b
  end

  def test_distinct_when_body_differs
    a = Kobako::Snippet::Source.new(name: :Helper, body: "X = 1")
    b = Kobako::Snippet::Source.new(name: :Helper, body: "Y = 2")

    refute_equal a, b
  end

  def test_instances_are_frozen
    source = Kobako::Snippet::Source.new(name: :Helper, body: "X = 1")

    assert_predicate source, :frozen?
  end
end
